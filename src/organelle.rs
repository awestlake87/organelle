use std;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use futures::prelude::*;
use futures::unsync::mpsc;
use tokio_core::reactor;

use super::{
    Effector,
    Handle,
    Impulse,
    Node,
    Result,
    Signal,
    Soma,
    SomaWrapper,
    Synapse,
};

struct OrganelleNodePool<S: Soma> {
    main_hdl: Handle,

    main: Box<Node<S::Signal, S::Synapse>>,

    misc: HashMap<Handle, Box<Node<S::Signal, S::Synapse>>>,
}

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
    parent: Option<Handle>,
    effector: Option<Effector<S::Signal, S::Synapse>>,

    main_hdl: Handle,
    connections: Vec<(Handle, Handle, S::Synapse)>,

    nodes: Rc<RefCell<OrganelleNodePool<S>>>,
}

impl<S: Soma + 'static> Organelle<S> {
    /// create a new organelle with input and output somas
    pub fn new(main: S) -> Self {
        let main_hdl = Handle::new_v4();

        Self {
            parent: None,
            effector: None,

            main_hdl: main_hdl,
            connections: vec![],

            nodes: Rc::from(RefCell::new(OrganelleNodePool {
                main_hdl: main_hdl,

                main: Box::new(SomaWrapper::new(main)),

                misc: HashMap::new(),
            })),
        }
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
        let node = Box::new(SomaWrapper::new(soma));
        let handle = Handle::new_v4();

        (*self.nodes).borrow_mut().misc.insert(handle, node);

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
        let mut nodes = (*self.nodes).borrow_mut();

        if hdl == nodes.main_hdl {
            nodes.main.update(msg)
        } else {
            nodes.misc.get_mut(&hdl).unwrap().update(msg)
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

        let main_hdl = self.main_hdl;

        self.update_node(
            main_hdl,
            Impulse::Init(
                Some(organelle_hdl),
                Effector {
                    this_soma: main_hdl,
                    sender: sender.clone(),
                    reactor: reactor.clone(),
                },
            ),
        )?;

        for (hdl, node) in (*self.nodes).borrow_mut().misc.iter_mut() {
            node.update(Impulse::Init(
                Some(organelle_hdl),
                Effector {
                    this_soma: *hdl,
                    sender: sender.clone(),
                    reactor: reactor.clone(),
                },
            ))?;
        }

        for &(input, output, role) in &self.connections {
            self.update_node(input, Impulse::AddOutput(output, role))?;
            self.update_node(output, Impulse::AddInput(input, role))?;
        }

        let external_sender = effector.sender;
        let nodes = Rc::clone(&self.nodes);
        let forward_reactor = reactor.clone();

        let stream_future = queue_rx.for_each(move |msg| {
            Self::forward(
                organelle_hdl,
                &mut (*nodes).borrow_mut(),
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
        {
            let mut nodes = (*self.nodes).borrow_mut();

            nodes.main.update(Impulse::Start)?;

            for node in nodes.misc.values_mut() {
                node.update(Impulse::Start)?;
            }
        }

        Ok(self)
    }

    fn add_input(self, input: Handle, role: S::Synapse) -> Result<Self> {
        (*self.nodes)
            .borrow_mut()
            .main
            .update(Impulse::AddInput(input, role))?;

        Ok(self)
    }

    fn add_output(self, output: Handle, role: S::Synapse) -> Result<Self> {
        (*self.nodes)
            .borrow_mut()
            .main
            .update(Impulse::AddOutput(output, role))?;

        Ok(self)
    }

    fn forward<T, U>(
        organelle: Handle,
        nodes: &mut OrganelleNodePool<S>,
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
                    if src == nodes.main_hdl {
                        // if src is the main node, then it becomes tricky.
                        // these are allowed to send to both internal and
                        // external somas, so the question becomes whether or
                        // not to advertise itself as the soma or the organelle

                        if dest == nodes.main_hdl
                            || nodes.misc.contains_key(&dest)
                        {
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

                if dest == nodes.main_hdl {
                    nodes.main.update(Impulse::Signal(actual_src, msg))?;
                } else if let Some(ref mut node) = nodes.misc.get_mut(&dest) {
                    // send to internal node
                    node.update(Impulse::Signal(actual_src, msg))?;
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

            Impulse::Probe(dest) => println!(
                "{}{:#?}",
                Self::type_name(),
                nodes
                    .misc
                    .iter()
                    .map(|(hdl, node)| (*hdl, node.type_name()))
                    .collect::<Vec<(Handle, &str)>>()
            ),

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
