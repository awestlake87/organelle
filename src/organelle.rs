
use std::cell::{ RefCell };
use std::collections::{ HashMap };
use std::fmt::Debug;
use std::hash::Hash;
use std::rc::Rc;

use futures::prelude::*;
use futures::sync::{ mpsc };
use tokio_core::reactor;

use super::{
    Result,
    Impulse,
    Handle,
    Effector,
    Soma,
    Node,
    SomaWrapper,
    SomaSynapse,
    SomaSignal,
};

struct OrganelleNodePool<S, Y> {
    main_hdl:       Handle,

    main:           Box<Node<S, Y>>,

    misc:           HashMap<Handle, Box<Node<S, Y>>>,
}

/// a special soma designed to contain a network of interconnected somas
///
/// the organelle is created with one soma. this soma is the only soma within the
/// organelle that is allowed to communicate or connect to the outside world. it
/// acts as an entry point for the network, providing essential external data
/// while keeping implementation-specific data and somas hidden. upon receiving
/// an update, it has the opportunity to communicate these updates with
/// external somas.
///
/// the intent is to allow organelles to be hierarchical and potentially contain
/// any number of nested soma networks. in order to do this, the organelle
/// isolates a group of messages from the larger whole. this is essential for
/// extensibility and maintainability.
///
/// any organelle can be plugged into any other organelle provided their messages and
/// dendrites can convert between each other using From and Into
pub struct Organelle<S, Y> where
    S: SomaSignal,
    Y: SomaSynapse
{
    effector:       Option<Effector<S, Y>>,

    main_hdl:       Handle,
    connections:    Vec<(Handle, Handle, Y)>,

    nodes:          Rc<RefCell<OrganelleNodePool<S, Y>>>,
}

impl<S, Y> Organelle<S, Y> where
    S: SomaSignal,
    Y: SomaSynapse,
{
    /// create a new organelle with input and output somas
    pub fn new<L>(main: L) -> Self where
        L: Soma<Signal=S, Synapse=Y> + 'static,
    {
        let main_hdl = Handle::new_v4();

        Self {
            effector: None,

            main_hdl: main_hdl,
            connections: vec![ ],

            nodes: Rc::from(
                RefCell::new(
                    OrganelleNodePool::<S, Y> {
                        main_hdl: main_hdl,

                        main: Box::new(SomaWrapper::new(main)),

                        misc: HashMap::new()
                    }
                )
            )
        }
    }


    /// add a new soma to the organelle and initialize it
    ///
    /// as long as the soma's message type can convert Into and From the
    /// organelle's message type, it can be added to the organelle and can
    /// communicate with any somas that do the same.
    pub fn add_soma<L>(&mut self, soma: L) -> Handle where
        L: Soma + 'static,

        S: From<L::Signal> + Into<L::Signal> + 'static,
        L::Signal: From<S> + Into<S> + 'static,

        Y: From<L::Synapse>
            + Into<L::Synapse>
            + Debug
            + Copy
            + Clone
            + Hash
            + Eq
            + PartialEq
            + 'static,

        L::Synapse: From<Y>
            + Into<Y>
            + Debug
            + Copy
            + Clone
            + Hash
            + Eq
            + PartialEq
            + 'static,
    {
        let node = Box::new(SomaWrapper::new(soma));
        let handle = Handle::new_v4();

        (*self.nodes).borrow_mut().misc.insert(handle, node);

        handle
    }

    /// connect input to output and update them accordingly
    pub fn connect(&mut self, input: Handle, output: Handle, role: Y) {
        self.connections.push((input, output, role));
    }

    /// get the main soma's handle
    pub fn get_main_handle(&self) -> Handle {
        self.main_hdl
    }

    fn update_node(&self, hdl: Handle, msg: Impulse<S, Y>) -> Result<()> {
        let mut nodes = (*self.nodes).borrow_mut();

        if hdl == nodes.main_hdl {
            nodes.main.update(msg)
        }
        else {
            nodes.misc.get_mut(&hdl).unwrap().update(msg)
        }
    }

    fn init<T, U>(mut self, effector: Effector<T, U>) -> Result<Self> where
        S: From<T> + Into<T> + SomaSignal,
        T: From<S> + Into<S> + SomaSignal,

        Y: From<U> + Into<U> + SomaSynapse,
        U: From<Y> + Into<Y> + SomaSynapse,
    {
        let organelle_hdl = effector.this_soma;

        let (queue_tx, queue_rx) = mpsc::channel(100);

        self.effector = Some(
            Effector {
                this_soma: organelle_hdl.clone(),
                sender: queue_tx,
                reactor: effector.reactor,
            }
        );

        let sender = self.effector
            .as_ref()
            .unwrap()
            .sender
            .clone()
        ;
        let reactor = self.effector
            .as_ref()
            .unwrap()
            .reactor
            .clone()
        ;

        let main_hdl = self.main_hdl;

        self.update_node(
            main_hdl,
            Impulse::Init(
                Effector {
                    this_soma: main_hdl,
                    sender: sender.clone(),
                    reactor: reactor.clone(),
                }
            )
        )?;

        for (hdl, node) in (*self.nodes).borrow_mut().misc.iter_mut() {
            node.update(
                Impulse::Init(
                    Effector {
                        this_soma: *hdl,
                        sender: sender.clone(),
                        reactor: reactor.clone(),
                    }
                )
            )?;
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
                msg
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

    fn add_input(self, input: Handle, role: Y) -> Result<Self> {
        (*self.nodes).borrow_mut().main.update
            (Impulse::AddInput(input, role)
        )?;

        Ok(self)
    }

    fn add_output(self, output: Handle, role: Y) -> Result<Self> {
        (*self.nodes).borrow_mut().main.update(
            Impulse::AddOutput(output, role)
        )?;

        Ok(self)
    }

    fn forward<T, U>(
        organelle: Handle,
        nodes: &mut OrganelleNodePool<S, Y>,
        sender: mpsc::Sender<Impulse<T, U>>,
        reactor: &reactor::Handle,
        msg: Impulse<S, Y>
    )
        -> Result<()> where
            S: From<T> + Into<T> + SomaSignal,
            T: From<S> + Into<S> + SomaSignal,

            Y: From<U> + Into<U> + SomaSynapse,
            U: From<Y> + Into<Y> + SomaSynapse,
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
                        }
                        else {
                            // external node - use organelle hdl
                            organelle
                        }
                    }
                    else {
                        src
                    }
                };

                if dest == nodes.main_hdl {
                    nodes.main.update(Impulse::Signal(actual_src, msg))?;
                }
                else if let Some(ref mut node) = nodes.misc.get_mut(&dest) {
                    // send to internal node
                    node.update(Impulse::Signal(actual_src, msg))?;
                }
                else {
                    // send to external node
                    reactor.spawn(
                        sender.send(
                            Impulse::<T, U>::convert_protocol(
                                Impulse::Payload(actual_src, dest, msg)
                            )
                        )
                            .then(|_| Ok(()))
                    );
                }
            },

            Impulse::Stop => reactor.spawn(
                sender.send(Impulse::Stop).then(|_| Ok(()))
            ),
            Impulse::Err(e) => reactor.spawn(
                sender.send(Impulse::Err(e)).then(|_| Ok(()))
            ),

            _ => unimplemented!()
        }

        Ok(())
    }
}

impl<S, Y> Soma for Organelle<S, Y> where
    S: SomaSignal,
    Y: SomaSynapse,
{
    type Signal = S;
    type Synapse = Y;

    fn update(self, msg: Impulse<S, Y>) -> Result<Self> {
        match msg {
            Impulse::Init(effector) => self.init(effector),
            Impulse::AddInput(input, role) => self.add_input(
                input, role
            ),
            Impulse::AddOutput(output, role) => self.add_output(
                output, role
            ),

            Impulse::Start => self.start(),
            Impulse::Signal(src, msg) => {
                self.update_node(self.main_hdl, Impulse::Signal(src, msg))?;

                Ok(self)
            },

            _ => unreachable!(),
        }
    }
}
