
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
    Protocol,
    Handle,
    Effector,
    Cell,
    Node,
    CellWrapper,
    CellRole,
    CellMessage,
};

struct OrganelleNodePool<M, R> {
    main_hdl:       Handle,

    main:           Box<Node<M, R>>,

    misc:           HashMap<Handle, Box<Node<M, R>>>,
}

/// a special cell designed to contain a network of interconnected cells
///
/// the organelle is created with one cell. this cell is the only cell within the
/// organelle that is allowed to communicate or connect to the outside world. it
/// acts as an entry point for the network, providing essential external data
/// while keeping implementation-specific data and cells hidden. upon receiving
/// an update, it has the opportunity to communicate these updates with
/// external cells.
///
/// the intent is to allow organelles to be hierarchical and potentially contain
/// any number of nested cell networks. in order to do this, the organelle
/// isolates a group of messages from the larger whole. this is essential for
/// extensibility and maintainability.
///
/// any organelle can be plugged into any other organelle provided their messages and
/// constraints can convert between each other using From and Into
pub struct Organelle<M, R> where
    M: CellMessage,
    R: CellRole
{
    effector:       Option<Effector<M, R>>,

    main_hdl:       Handle,
    connections:    Vec<(Handle, Handle, R)>,

    nodes:          Rc<RefCell<OrganelleNodePool<M, R>>>,
}

impl<M, R> Organelle<M, R> where
    M: CellMessage,
    R: CellRole,
{
    /// create a new organelle with input and output cells
    pub fn new<L>(main: L) -> Self where
        L: Cell<Message=M, Role=R> + 'static,
    {
        let main_hdl = Handle::new_v4();

        Self {
            effector: None,

            main_hdl: main_hdl,
            connections: vec![ ],

            nodes: Rc::from(
                RefCell::new(
                    OrganelleNodePool::<M, R> {
                        main_hdl: main_hdl,

                        main: Box::new(CellWrapper::new(main)),

                        misc: HashMap::new()
                    }
                )
            )
        }
    }


    /// add a new cell to the organelle and initialize it
    ///
    /// as long as the cell's message type can convert Into and From the
    /// organelle's message type, it can be added to the organelle and can
    /// communicate with any cells that do the same.
    pub fn add_cell<L>(&mut self, cell: L) -> Handle where
        L: Cell + 'static,

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
        let node = Box::new(CellWrapper::new(cell));
        let handle = Handle::new_v4();

        (*self.nodes).borrow_mut().misc.insert(handle, node);

        handle
    }

    /// connect input to output and update them accordingly
    pub fn connect(&mut self, input: Handle, output: Handle, role: R) {
        self.connections.push((input, output, role));
    }

    /// get the main cell's handle
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
        M: From<T> + Into<T> + CellMessage,
        T: From<M> + Into<M> + CellMessage,

        R: From<U> + Into<U> + CellRole,
        U: From<R> + Into<R> + CellRole,
    {
        let organelle_hdl = effector.this_cell;

        let (queue_tx, queue_rx) = mpsc::channel(100);

        self.effector = Some(
            Effector {
                this_cell: organelle_hdl.clone(),
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
            Protocol::Init(
                Effector {
                    this_cell: main_hdl,
                    sender: sender.clone(),
                    reactor: reactor.clone(),
                }
            )
        )?;

        for (hdl, node) in (*self.nodes).borrow_mut().misc.iter_mut() {
            node.update(
                Protocol::Init(
                    Effector {
                        this_cell: *hdl,
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
        organelle: Handle,
        nodes: &mut OrganelleNodePool<M, R>,
        sender: mpsc::Sender<Protocol<T, U>>,
        reactor: &reactor::Handle,
        msg: Protocol<M, R>
    )
        -> Result<()> where
            M: From<T> + Into<T> + CellMessage,
            T: From<M> + Into<M> + CellMessage,

            R: From<U> + Into<U> + CellRole,
            U: From<R> + Into<R> + CellRole,
    {
        match msg {
            Protocol::Payload(src, dest, msg) => {
                let actual_src = {
                    // check if src is the main cell
                    if src == nodes.main_hdl {
                        // if src is the main node, then it becomes tricky.
                        // these are allowed to send to both internal and
                        // external cells, so the question becomes whether or
                        // not to advertise itself as the cell or the organelle

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
                    nodes.main.update(Protocol::Message(actual_src, msg))?;
                }
                else if let Some(ref mut node) = nodes.misc.get_mut(&dest) {
                    // send to internal node
                    node.update(Protocol::Message(actual_src, msg))?;
                }
                else {
                    // send to external node
                    reactor.spawn(
                        sender.send(
                            Protocol::<T, U>::convert_protocol(
                                Protocol::Payload(actual_src, dest, msg)
                            )
                        )
                            .then(|_| Ok(()))
                    );
                }
            },

            Protocol::Stop => reactor.spawn(
                sender.send(Protocol::Stop).then(|_| Ok(()))
            ),
            Protocol::Err(e) => reactor.spawn(
                sender.send(Protocol::Err(e)).then(|_| Ok(()))
            ),

            _ => unimplemented!()
        }

        Ok(())
    }
}

impl<M, R> Cell for Organelle<M, R> where
    M: CellMessage,
    R: CellRole,
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
