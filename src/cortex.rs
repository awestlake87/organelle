
use std::cell::{ RefCell };
use std::collections::{ HashMap };
use std::rc::Rc;

use futures::prelude::*;
use futures::sync::{ mpsc };
use tokio_core::reactor;

use super::{ Result, Protocol, Handle, Effector, Lobe, Node, LobeWrapper };

struct CortexNodePool<M, C> {
    input_hdl:      Handle,
    output_hdl:     Handle,

    input:          Box<Node<M, C>>,
    output:         Box<Node<M, C>>,

    misc:           HashMap<Handle, Box<Node<M, C>>>,
}

/// a special lobe designed to contain a network of interconnected lobes
///
/// the cortex is created with one input lobe and one output lobe. these lobes
/// are special in that they are the only lobes within the cortex that are
/// allowed to communicate or connect to the outside world. the input node can
/// act as an entry point for the network, providing essential external data
/// while keeping implementation-specific data and lobes hidden. upon receiving
/// an update, the output node has the opportunity to communicate these updates
/// with external lobes.
///
/// the intent is to allow cortices to be hierarchical and potentially contain
/// any number of nested lobe networks. in order to do this, the cortex
/// isolates a group of messages from the larger whole. this is essential for
/// extensibility and maintainability.
///
/// any cortex can be plugged into any other cortex provided their messages can
/// convert between each other using From and Into
pub struct Cortex<M: 'static, C: Copy + Clone + Eq + PartialEq + 'static> {
    effector:       Option<Effector<M, C>>,

    input_hdl:      Handle,
    output_hdl:     Handle,
    connections:    Vec<(Handle, Handle, C)>,

    nodes:          Rc<RefCell<CortexNodePool<M, C>>>,
}

impl<M, C> Cortex<M, C> where
    M: 'static,
    C: Copy + Clone + Eq + PartialEq + 'static,
{
    /// create a new cortex with input and output lobes
    pub fn new<I, O, IM, OM, IC, OC>(input: I, output: O) -> Self where
        M: From<IM> + Into<IM> + From<OM> + Into<OM> + 'static,
        C: From<IC> + Into<IC> + From<OC> + Into<OC> + 'static,

        I: Lobe<Message=IM, Constraint=IC> + 'static,
        O: Lobe<Message=OM, Constraint=OC> + 'static,

        IM: From<M> + Into<M> + 'static,
        OM: From<M> + Into<M> + 'static,

        IC: From<C> + Into<C> + Copy + Clone + Eq + PartialEq + 'static,
        OC: From<C> + Into<C> + Copy + Clone + Eq + PartialEq + 'static,
    {
        let input_hdl = Handle::new_v4();
        let output_hdl = Handle::new_v4();

        Self {
            effector: None,

            input_hdl: input_hdl,
            output_hdl: output_hdl,
            connections: vec![ ],

            nodes: Rc::from(
                RefCell::new(
                    CortexNodePool::<M, C> {
                        input_hdl: input_hdl,
                        output_hdl: output_hdl,

                        input: Box::new(LobeWrapper::new(input)),
                        output: Box::new(LobeWrapper::new(output)),

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
    pub fn add_lobe<L, T, U>(&mut self, lobe: L) -> Handle where
        L: Lobe<Message=T, Constraint=U> + 'static,

        M: From<T> + Into<T> + 'static,
        T: From<M> + Into<M> + 'static,

        C: From<U> + Into<U> + Copy + Clone + Eq + PartialEq + 'static,
        U: From<C> + Into<C> + Copy + Clone + Eq + PartialEq + 'static,
    {
        let node = Box::new(LobeWrapper::new(lobe));
        let handle = Handle::new_v4();

        (*self.nodes).borrow_mut().misc.insert(handle, node);

        handle
    }

    /// connect input to output and update them accordingly
    pub fn connect(&mut self, input: Handle, output: Handle, constraint: C) {
        self.connections.push((input, output, constraint));
    }

    /// get the input lobe's handle
    pub fn get_input(&self) -> Handle {
        self.input_hdl
    }

    /// get the output lobe's handle
    pub fn get_output(&self) -> Handle {
        self.output_hdl
    }

    fn update_node(&self, hdl: Handle, msg: Protocol<M, C>) -> Result<()> {
        let mut nodes = (*self.nodes).borrow_mut();

        if hdl == nodes.input_hdl {
            nodes.input.update(msg)
        }
        else if hdl == nodes.output_hdl {
            nodes.output.update(msg)
        }
        else {
            nodes.misc.get_mut(&hdl).unwrap().update(msg)
        }
    }

    fn init<T, U>(mut self, effector: Effector<T, U>) -> Result<Self> where
        M: From<T> + Into<T> + 'static,
        T: From<M> + Into<M> + 'static,

        C: From<U> + Into<U> + Copy + Clone + Eq + PartialEq,
        U: From<C> + Into<C> + Copy + Clone + Eq + PartialEq,
    {
        let cortex_hdl = effector.handle;

        let (queue_tx, queue_rx) = mpsc::channel(100);

        self.effector = Some(
            Effector {
                handle: cortex_hdl.clone(),
                sender: Rc::from(
                    move |r: &reactor::Handle, msg: Protocol<M, C>| r.spawn(
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

        let input_hdl = self.input_hdl;
        let output_hdl = self.output_hdl;

        self.update_node(
            input_hdl,
            Protocol::Init(
                Effector {
                    handle: input_hdl,
                    sender: sender.clone(),
                    reactor: reactor.clone(),
                }
            )
        )?;
        self.update_node(
            output_hdl,
            Protocol::Init(
                Effector {
                    handle: output_hdl,
                    sender: sender.clone(),
                    reactor: reactor.clone(),
                }
            )
        )?;

        for node in (*self.nodes).borrow_mut().misc.values_mut() {
            node.update(
                Protocol::Init(
                    Effector {
                        handle: Handle::new_v4(),
                        sender: sender.clone(),
                        reactor: reactor.clone(),
                    }
                )
            )?;
        }

        for &(input, output, constraint) in &self.connections {
            self.update_node(input, Protocol::AddOutput(output, constraint))?;
            self.update_node(output, Protocol::AddInput(input, constraint))?;
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

            nodes.input.update(Protocol::Start)?;
            nodes.output.update(Protocol::Start)?;

            for node in nodes.misc.values_mut() {
                node.update(Protocol::Start)?;
            }
        }

        Ok(self)
    }

    fn add_input(self, input: Handle, constraint: C) -> Result<Self> {
        (*self.nodes).borrow_mut().input.update
            (Protocol::AddInput(input, constraint)
        )?;

        Ok(self)
    }

    fn add_output(self, output: Handle, constraint: C) -> Result<Self> {
        (*self.nodes).borrow_mut().output.update(
            Protocol::AddOutput(output, constraint)
        )?;

        Ok(self)
    }

    fn forward<T, U>(
        cortex: Handle,
        nodes: &mut CortexNodePool<M, C>,
        sender: &Fn(&reactor::Handle, Protocol<T, U>),
        reactor: &reactor::Handle,
        msg: Protocol<M, C>
    )
        -> Result<()> where
            M: From<T> + Into<T> + 'static,
            T: From<M> + Into<M> + 'static,

            C: From<U> + Into<U> + Copy + Clone + Eq + PartialEq,
            U: From<C> + Into<C> + Copy + Clone + Eq + PartialEq,
    {
        match msg {
            Protocol::Payload(src, dest, msg) => {
                let actual_src = {
                    // check if src is output or input
                    if src == nodes.output_hdl || src == nodes.input_hdl {
                        // if src is a special node, then it becomes tricky.
                        // these are allowed to send to both internal and
                        // external nodes, so the question becomes whether or
                        // not to advertise itself as the node or the cortex

                        if dest == nodes.input_hdl
                            || dest == nodes.output_hdl
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

                if dest == nodes.input_hdl {
                    nodes.input.update(Protocol::Message(actual_src, msg))?;
                }
                else if dest == nodes.output_hdl {
                    nodes.output.update(Protocol::Message(actual_src, msg))?;
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

            _ => unimplemented!()
        }

        Ok(())
    }
}

impl<M, C> Lobe for Cortex<M, C> where
    M: 'static,
    C: Copy + Clone + Eq + PartialEq + 'static,
{
    type Message = M;
    type Constraint = C;

    fn update(self, msg: Protocol<M, C>) -> Result<Self> {
        match msg {
            Protocol::Init(effector) => self.init(effector),
            Protocol::AddInput(input, constraint) => self.add_input(
                input, constraint
            ),
            Protocol::AddOutput(output, constraint) => self.add_output(
                output, constraint
            ),

            Protocol::Start => self.start(),
            Protocol::Message(src, msg) => {
                self.update_node(
                    self.input_hdl,
                    Protocol::Message(src, msg)
                )?;

                Ok(self)
            },

            _ => unreachable!(),
        }
    }
}
