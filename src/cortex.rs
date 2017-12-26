
use std::cell::{ RefCell };
use std::collections::{ HashMap };
use std::fmt::Debug;
use std::hash::Hash;
use std::rc::Rc;

use futures::prelude::*;
use futures::sync::{ mpsc };
use tokio_core::reactor;

use super::{ Result, Protocol, Handle, Effector, Lobe, Node, LobeWrapper };

struct CortexNodePool<M, R> {
    main_hdl:       Handle,

    main:           Box<Node<M, R>>,

    misc:           HashMap<Handle, Box<Node<M, R>>>,
}

/// a special lobe designed to contain a network of interconnected lobes
///
/// the cortex is created with one lobe. this lobe is the only lobe within the
/// cortex that is allowed to communicate or connect to the outside world. it
/// acts as an entry point for the network, providing essential external data
/// while keeping implementation-specific data and lobes hidden. upon receiving
/// an update, it has the opportunity to communicate these updates with
/// external lobes.
///
/// the intent is to allow cortices to be hierarchical and potentially contain
/// any number of nested lobe networks. in order to do this, the cortex
/// isolates a group of messages from the larger whole. this is essential for
/// extensibility and maintainability.
///
/// any cortex can be plugged into any other cortex provided their messages and
/// constraints can convert between each other using From and Into
pub struct Cortex<M, R> where
    M: 'static,
    R: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static
{
    effector:       Option<Effector<M, R>>,

    main_hdl:       Handle,
    connections:    Vec<(Handle, Handle, R)>,

    nodes:          Rc<RefCell<CortexNodePool<M, R>>>,
}

impl<M, R> Cortex<M, R> where
    M: 'static,
    R: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static,
{
    /// create a new cortex with input and output lobes
    pub fn new<L>(main: L) -> Self where
        L: Lobe<Message=M, Role=R> + 'static,
    {
        let main_hdl = Handle::new_v4();

        Self {
            effector: None,

            main_hdl: main_hdl,
            connections: vec![ ],

            nodes: Rc::from(
                RefCell::new(
                    CortexNodePool::<M, R> {
                        main_hdl: main_hdl,

                        main: Box::new(LobeWrapper::new(main)),

                        misc: HashMap::new()
                    }
                )
            )
        }
    }


    /// add a new lobe to the cortex and initialize it
    ///
    /// as long as the lobe's message type can convert Into and From the
    /// cortex's message type, it can be added to the cortex and can
    /// communicate with any lobes that do the same.
    pub fn add_lobe<L>(&mut self, lobe: L) -> Handle where
        L: Lobe + 'static,

        M: From<L::Message> + Into<L::Message> + 'static,
        L::Message: From<M> + Into<M> + 'static,

        R: From<L::Role>
            + Into<L::Role>
            + Debug
            + Copy
            + Clone
            + Hash
            + Eq
            + PartialEq
            + 'static,

        L::Role: From<R>
            + Into<R>
            + Debug
            + Copy
            + Clone
            + Hash
            + Eq
            + PartialEq
            + 'static,
    {
        let node = Box::new(LobeWrapper::new(lobe));
        let handle = Handle::new_v4();

        (*self.nodes).borrow_mut().misc.insert(handle, node);

        handle
    }

    /// connect input to output and update them accordingly
    pub fn connect(&mut self, input: Handle, output: Handle, role: R) {
        self.connections.push((input, output, role));
    }

    /// get the main lobe's handle
    pub fn get_main_handle(&self) -> Handle {
        self.main_hdl
    }

    fn update_node(&self, hdl: Handle, msg: Protocol<M, R>) -> Result<()> {
        let mut nodes = (*self.nodes).borrow_mut();

        if hdl == nodes.main_hdl {
            nodes.main.update(msg)
        }
        else {
            nodes.misc.get_mut(&hdl).unwrap().update(msg)
        }
    }

    fn init<T, U>(mut self, effector: Effector<T, U>) -> Result<Self> where
        M: From<T> + Into<T> + 'static,
        T: From<M> + Into<M> + 'static,

        R: From<U> + Into<U> + Debug + Copy + Clone + Hash + Eq + PartialEq,
        U: From<R> + Into<R> + Debug + Copy + Clone + Hash + Eq + PartialEq,
    {
        let cortex_hdl = effector.this_lobe;

        let (queue_tx, queue_rx) = mpsc::channel(100);

        self.effector = Some(
            Effector {
                this_lobe: cortex_hdl.clone(),
                sender: Rc::from(
                    move |r: &reactor::Handle, msg: Protocol<M, R>| r.spawn(
                        queue_tx.clone().send(msg)
                           .then(
                               |result| match result {
                                   Ok(_) => Ok(()),
                                   Err(_) => Ok(())
                               }
                           )
                    )
                ),
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
            Protocol::Init(
                Effector {
                    this_lobe: main_hdl,
                    sender: sender.clone(),
                    reactor: reactor.clone(),
                }
            )
        )?;

        for (hdl, node) in (*self.nodes).borrow_mut().misc.iter_mut() {
            node.update(
                Protocol::Init(
                    Effector {
                        this_lobe: *hdl,
                        sender: sender.clone(),
                        reactor: reactor.clone(),
                    }
                )
            )?;
        }

        for &(input, output, role) in &self.connections {
            self.update_node(input, Protocol::AddOutput(output, role))?;
            self.update_node(output, Protocol::AddInput(input, role))?;
        }

        let external_sender = effector.sender;
        let nodes = Rc::clone(&self.nodes);
        let forward_reactor = reactor.clone();

        let stream_future = queue_rx.for_each(
            move |msg| {
                Self::forward(
                    cortex_hdl,
                    &mut (*nodes).borrow_mut(),
                    &*external_sender,
                    &forward_reactor,
                    msg
                ).unwrap();

                Ok(())
            }
        );

        reactor.spawn(stream_future);

        Ok(self)
    }

    fn start(self) -> Result<Self> {
        {
            let mut nodes = (*self.nodes).borrow_mut();

            nodes.main.update(Protocol::Start)?;

            for node in nodes.misc.values_mut() {
                node.update(Protocol::Start)?;
            }
        }

        Ok(self)
    }

    fn add_input(self, input: Handle, role: R) -> Result<Self> {
        (*self.nodes).borrow_mut().main.update
            (Protocol::AddInput(input, role)
        )?;

        Ok(self)
    }

    fn add_output(self, output: Handle, role: R) -> Result<Self> {
        (*self.nodes).borrow_mut().main.update(
            Protocol::AddOutput(output, role)
        )?;

        Ok(self)
    }

    fn forward<T, U>(
        cortex: Handle,
        nodes: &mut CortexNodePool<M, R>,
        sender: &Fn(&reactor::Handle, Protocol<T, U>),
        reactor: &reactor::Handle,
        msg: Protocol<M, R>
    )
        -> Result<()> where
            M: From<T> + Into<T> + 'static,
            T: From<M> + Into<M> + 'static,

            R: From<U>
                + Into<U>
                + Debug
                + Copy
                + Clone
                + Hash
                + Eq
                + PartialEq,

            U: From<R>
                + Into<R>
                + Debug
                + Copy
                + Clone
                + Hash
                + Eq
                + PartialEq,
    {
        match msg {
            Protocol::Payload(src, dest, msg) => {
                let actual_src = {
                    // check if src is the main lobe
                    if src == nodes.main_hdl {
                        // if src is the main node, then it becomes tricky.
                        // these are allowed to send to both internal and
                        // external lobes, so the question becomes whether or
                        // not to advertise itself as the lobe or the cortex

                        if dest == nodes.main_hdl
                            || nodes.misc.contains_key(&dest)
                        {
                            // internal node - use src
                            src
                        }
                        else {
                            // external node - use cortex hdl
                            cortex
                        }
                    }
                    else {
                        src
                    }
                };

                if dest == nodes.main_hdl {
                    nodes.main.update(Protocol::Message(actual_src, msg))?;
                }
                else if let Some(ref mut node) = nodes.misc.get_mut(&dest) {
                    // send to internal node
                    node.update(Protocol::Message(actual_src, msg))?;
                }
                else {
                    // send to external node
                    sender(
                        reactor,
                        Protocol::<T, U>::convert_protocol(
                            Protocol::Payload(actual_src, dest, msg)
                        )
                    );
                }
            },

            Protocol::Stop => sender(reactor, Protocol::Stop),
            Protocol::Err(e) => sender(reactor, Protocol::Err(e)),

            _ => unimplemented!()
        }

        Ok(())
    }
}

impl<M, R> Lobe for Cortex<M, R> where
    M: 'static,
    R: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static,
{
    type Message = M;
    type Role = R;

    fn update(self, msg: Protocol<M, R>) -> Result<Self> {
        match msg {
            Protocol::Init(effector) => self.init(effector),
            Protocol::AddInput(input, role) => self.add_input(
                input, role
            ),
            Protocol::AddOutput(output, role) => self.add_output(
                output, role
            ),

            Protocol::Start => self.start(),
            Protocol::Message(src, msg) => {
                self.update_node(self.main_hdl, Protocol::Message(src, msg))?;

                Ok(self)
            },

            _ => unreachable!(),
        }
    }
}
