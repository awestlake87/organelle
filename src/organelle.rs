use std;
use std::collections::HashMap;
use std::mem;

use futures::future;
use futures::prelude::*;
use futures::unsync::mpsc;
use tokio_core::reactor;

use super::{
    Effector,
    Error,
    ErrorKind,
    Handle,
    Impulse,
    Result,
    Signal,
    Soma,
    Synapse,
};

/// a special soma designed to contain a network of interconnected somas
///
/// the organelle is created with one soma. this soma is the only soma within
/// the organelle that is allowed to communicate or connect to the outside
/// world. it acts as an entry point for the network, providing essential
/// external data while keeping implementation-specific data and somas hidden.
/// upon receiving an update, it has the opportunity to communicate these
/// updates with external somas.
///
/// the intent is to allow organelles to be hierarchical and potentially contain
/// any number of nested soma networks. in order to do this, the organelle
/// isolates a group of messages from the larger whole. this is essential for
/// extensibility and maintainability.
///
/// any organelle can be plugged into any other organelle provided their
/// messages and dendrites can convert between each other using From and Into
pub struct Organelle<S: Soma + 'static> {
    reactor: reactor::Handle,
    sender: mpsc::Sender<Impulse<S::Signal, S::Synapse>>,
    receiver: Option<mpsc::Receiver<Impulse<S::Signal, S::Synapse>>>,

    parent: Option<Handle>,
    effector: Option<Effector<S::Signal, S::Synapse>>,

    main_hdl: Handle,
    connections: Vec<(Handle, Handle, S::Synapse)>,

    nodes: HashMap<Handle, mpsc::Sender<Impulse<S::Signal, S::Synapse>>>,
}

impl<S: Soma + 'static> Organelle<S> {
    /// create a new organelle with input and output somas
    pub fn new(reactor: reactor::Handle, main: S) -> Self {
        let (tx, rx) = mpsc::channel(10);

        let mut organelle = Self {
            reactor: reactor,
            sender: tx,
            receiver: Some(rx),

            parent: None,
            effector: None,

            // temporary, gets overwritten below
            main_hdl: Handle::new_v4(),
            connections: vec![],

            nodes: HashMap::new(),
        };

        let main_hdl = organelle.add_soma(main);
        organelle.main_hdl = main_hdl;

        organelle
    }

    /// add a new soma to the organelle and initialize it
    ///
    /// as long as the soma's message type can convert Into and From the
    /// organelle's message type, it can be added to the organelle and can
    /// communicate with any somas that do the same.
    pub fn add_soma<T>(&mut self, soma: T) -> Handle
    where
        T: Soma + 'static,

        S::Signal: From<T::Signal> + Into<T::Signal> + Signal,
        T::Signal: From<S::Signal> + Into<S::Signal> + Signal,

        S::Synapse: From<T::Synapse> + Into<T::Synapse> + Synapse,
        T::Synapse: From<S::Synapse> + Into<S::Synapse> + Synapse,
    {
        let handle = Handle::new_v4();
        let organelle_sender = self.sender.clone();
        let reactor = self.reactor.clone();

        let (tx, rx) = mpsc::channel(10);

        self.reactor.spawn(rx.fold(
            Some(soma),
            move |soma: Option<T>, imp: Impulse<S::Signal, S::Synapse>| {
                if soma.is_none() {
                    return future::ok(None);
                }

                let result = soma.unwrap().update(
                    Impulse::<T::Signal, T::Synapse>::convert_protocol(imp),
                );

                match result {
                    Ok(soma) => future::ok(Some(soma)),
                    Err(e) => {
                        reactor.spawn(
                            organelle_sender
                                .clone()
                                .send(Impulse::Err(Error::with_chain(
                                    e,
                                    ErrorKind::SomaError,
                                )))
                                .then(|_| future::ok(())),
                        );

                        future::ok(None)
                    },
                }
            },
        ).then(|_| future::ok(())));

        self.nodes.insert(handle, tx);

        handle
    }

    /// connect input to output and update them accordingly
    pub fn connect(&mut self, input: Handle, output: Handle, role: S::Synapse) {
        self.connections.push((input, output, role));
    }

    /// get the main soma's handle
    pub fn get_main_handle(&self) -> Handle {
        self.main_hdl
    }

    fn update_node(
        &self,
        hdl: Handle,
        msg: Impulse<S::Signal, S::Synapse>,
    ) -> Result<()> {
        if let Some(sender) = self.nodes.get(&hdl) {
            self.reactor
                .spawn(sender.clone().send(msg).then(|_| future::ok(())));

            Ok(())
        } else {
            bail!("node not found")
        }
    }

    fn init<T, U>(
        mut self,
        parent: Option<Handle>,
        effector: Effector<T, U>,
    ) -> Result<Self>
    where
        S::Signal: From<T> + Into<T> + Signal,
        T: From<S::Signal> + Into<S::Signal> + Signal,

        S::Synapse: From<U> + Into<U> + Synapse,
        U: From<S::Synapse> + Into<S::Synapse> + Synapse,
    {
        self.parent = parent;

        let organelle_hdl = effector.this_soma;
        let (queue_tx, queue_rx) = mpsc::channel(100);

        self.effector = Some(Effector {
            this_soma: organelle_hdl.clone(),
            sender: queue_tx,
            reactor: effector.reactor,
        });

        let sender = self.effector.as_ref().unwrap().sender.clone();
        let reactor = self.effector.as_ref().unwrap().reactor.clone();

        for (hdl, _) in &self.nodes {
            self.update_node(
                *hdl,
                Impulse::Init(
                    Some(organelle_hdl),
                    Effector {
                        this_soma: *hdl,
                        sender: sender.clone(),
                        reactor: reactor.clone(),
                    },
                ),
            )?;
        }

        for &(input, output, role) in &self.connections {
            self.update_node(input, Impulse::AddOutput(output, role))?;
            self.update_node(output, Impulse::AddInput(input, role))?;
        }

        let main_hdl = self.main_hdl;
        let external_sender = effector.sender;
        let nodes = self.nodes.clone();
        let forward_reactor = reactor.clone();

        let stream_future = queue_rx.for_each(move |msg| {
            Self::forward(
                organelle_hdl,
                main_hdl,
                &nodes,
                external_sender.clone(),
                &forward_reactor,
                msg,
            ).unwrap();

            Ok(())
        });

        reactor.spawn(stream_future);

        Ok(self)
    }

    fn start(self) -> Result<Self> {
        for node in self.nodes.keys() {
            self.update_node(*node, Impulse::Start)?;
        }

        Ok(self)
    }

    fn add_input(self, input: Handle, role: S::Synapse) -> Result<Self> {
        self.update_node(self.main_hdl, Impulse::AddInput(input, role))?;

        Ok(self)
    }

    fn add_output(self, output: Handle, role: S::Synapse) -> Result<Self> {
        self.update_node(self.main_hdl, Impulse::AddOutput(output, role))?;

        Ok(self)
    }

    fn forward<T, U>(
        organelle: Handle,
        main_hdl: Handle,
        nodes: &HashMap<Handle, mpsc::Sender<Impulse<S::Signal, S::Synapse>>>,
        sender: mpsc::Sender<Impulse<T, U>>,
        reactor: &reactor::Handle,
        msg: Impulse<S::Signal, S::Synapse>,
    ) -> Result<()>
    where
        S::Signal: From<T> + Into<T> + Signal,
        T: From<S::Signal> + Into<S::Signal> + Signal,

        S::Synapse: From<U> + Into<U> + Synapse,
        U: From<S::Synapse> + Into<S::Synapse> + Synapse,
    {
        match msg {
            Impulse::Payload(src, dest, msg) => {
                let actual_src = {
                    // check if src is the main soma
                    if src == main_hdl {
                        // if src is the main node, then it becomes tricky.
                        // these are allowed to send to both internal and
                        // external somas, so the question becomes whether or
                        // not to advertise itself as the soma or the organelle

                        if dest == organelle || nodes.contains_key(&dest) {
                            // internal node - use src
                            src
                        } else {
                            // external node - use organelle hdl
                            organelle
                        }
                    } else {
                        src
                    }
                };

                if dest == organelle {
                    if let Some(main) = nodes.get(&main_hdl) {
                        reactor.spawn(
                            main.clone()
                                .send(Impulse::Signal(actual_src, msg))
                                .then(|_| future::ok(())),
                        );
                    } else {
                        bail!("main soma not found")
                    }
                } else if let Some(soma) = nodes.get(&dest) {
                    // send to internal node
                    reactor.spawn(
                        soma.clone()
                            .send(Impulse::Signal(actual_src, msg))
                            .then(|_| future::ok(())),
                    );
                } else {
                    // send to external node
                    reactor.spawn(
                        sender
                            .send(Impulse::<T, U>::convert_protocol(
                                Impulse::Payload(actual_src, dest, msg),
                            ))
                            .then(|_| Ok(())),
                    );
                }
            },

            Impulse::Probe(_) => println!("{}", Self::type_name()),

            Impulse::Stop => {
                reactor.spawn(sender.send(Impulse::Stop).then(|_| Ok(())))
            },
            Impulse::Err(e) => {
                reactor.spawn(sender.send(Impulse::Err(e)).then(|_| Ok(())))
            },

            _ => unimplemented!(),
        }

        Ok(())
    }
}

impl<S: Soma> Soma for Organelle<S> {
    type Signal = S::Signal;
    type Synapse = S::Synapse;
    type Error = S::Error;

    fn update(
        self,
        msg: Impulse<S::Signal, S::Synapse>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(match msg {
            Impulse::Init(parent, effector) => self.init(parent, effector)?,
            Impulse::AddInput(input, role) => self.add_input(input, role)?,
            Impulse::AddOutput(output, role) => self.add_output(output, role)?,

            Impulse::Start => self.start()?,
            Impulse::Signal(src, msg) => {
                self.update_node(self.main_hdl, Impulse::Signal(src, msg))?;

                self
            },

            _ => unreachable!(),
        })
    }
}

impl<S: Soma + 'static> IntoFuture for Organelle<S> {
    type Item = ();
    type Error = Error;
    type Future = Box<Future<Item = Self::Item, Error = Self::Error>>;

    /// convert the soma into a future that can be run on an event loop
    fn into_future(mut self) -> Self::Future {
        let (queue_tx, queue_rx) = (
            self.sender.clone(),
            mem::replace(&mut self.receiver, None).unwrap(),
        );

        let main_soma = Handle::new_v4();

        let sender = queue_tx.clone();

        let reactor_copy = self.reactor.clone();

        self.reactor.clone().spawn(
            queue_tx
                .clone()
                .send(Impulse::Init(
                    None,
                    Effector {
                        this_soma: main_soma,
                        sender: sender,
                        reactor: self.reactor.clone(),
                    },
                ))
                .and_then(|tx| {
                    tx.send(Impulse::Start).then(|result| match result {
                        Ok(_) => Ok(()),
                        Err(e) => panic!("unable to start main soma: {:?}", e),
                    })
                })
                .then(|result| match result {
                    Ok(_) => Ok(()),
                    Err(e) => panic!("unable to initialize main soma: {:?}", e),
                }),
        );

        let (tx, rx) = mpsc::channel::<Error>(1);

        let stream_future = queue_rx
            .take_while(|imp| match *imp {
                Impulse::Stop => Ok(false),
                _ => Ok(true),
            })
            .fold(Some(self), move |soma, imp| {
                let result =
                    match imp {
                        Impulse::Init(parent, effector) => soma.unwrap()
                            .update(Impulse::Init(parent, effector)),
                        Impulse::AddInput(input, role) => {
                            soma.unwrap().update(Impulse::AddInput(input, role))
                        },
                        Impulse::AddOutput(output, role) => soma.unwrap()
                            .update(Impulse::AddOutput(output, role)),

                        Impulse::Start => soma.unwrap().update(Impulse::Start),

                        Impulse::Payload(src, dest, msg) => {
                            // messages should only be sent to our soma
                            assert_eq!(dest, main_soma);

                            soma.unwrap().update(Impulse::Signal(src, msg))
                        },
                        Impulse::Probe(dest) => {
                            // probes should only be send to our soma
                            assert_eq!(dest, main_soma);
                            soma.unwrap().update(Impulse::Probe(dest))
                        },

                        Impulse::Err(e) => Err(e.into()),

                        _ => unreachable!(),
                    };

                match result {
                    Ok(soma) => future::ok(Some(soma)),
                    Err(e) => {
                        reactor_copy.spawn(
                            tx.clone()
                                .send(Error::with_chain(
                                    e,
                                    ErrorKind::SomaError,
                                ))
                                .then(|result| match result {
                                    Ok(_) => Ok(()),
                                    Err(e) => {
                                        panic!("unable to send error: {:?}", e)
                                    },
                                }),
                        );

                        future::ok(None)
                    },
                }
            })
            .then(move |_| {
                // make sure the channel stays open
                let _keep_alive = queue_tx;

                Ok(())
            });

        Box::new(
            stream_future
                .map(|_| Ok(()))
                .select(
                    rx.into_future()
                        .map(|(item, _)| Err(item.unwrap()))
                        .map_err(|_| ()),
                )
                .map(|(result, _)| result)
                .map_err(|_| -> Error {
                    ErrorKind::Msg("select error".into()).into()
                })
                .then(|result| match result {
                    Ok(Ok(())) => future::ok(()),
                    Ok(Err(e)) => future::err(e),
                    Err(e) => future::err(e),
                }),
        )
    }
}
