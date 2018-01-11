#![warn(missing_docs)]

//! Organelle - reactive architecture for emergent AI systems

#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate tokio;
extern crate tokio_core;
extern crate uuid;

mod organelle;
mod cell;
mod soma;

pub use organelle::{ Organelle };
pub use cell::{ Cell, CellMessage, CellRole };
pub use soma::{ Soma, Constraint, Nucleus, Eukaryote };

use std::fmt::Debug;
use std::hash::Hash;
use std::marker::PhantomData;
use std::mem;

use futures::prelude::*;
use futures::stream::iter_ok;
use futures::sync::mpsc;
use tokio_core::reactor;
use uuid::Uuid;

/// organelle error
error_chain! {
    foreign_links {
        Io(std::io::Error) #[doc = "glue for io::Error"];
        Canceled(futures::Canceled) #[doc = "glue for futures::Canceled"];
    }
    errors {
        /// a cell returned an error when called into
        CellError {
            description("an error occurred while calling into a cell"),
            display("an error occurred while calling into a cell")
        }
    }
}

/// handle to a cell within the organelle
pub type Handle = Uuid;

/// a set of protocol messages to be relayed throughout the network
///
/// wraps a user-defined message within the organelle protocol.
/// 1. a cell is always updated with Init first.
/// 2. as the organelle is built, the cell will be updated with any inputs or
///     outputs specified using AddInput and AddOutput.
/// 3. when the organelle is ready to begin execution, every cell is updated with
///     Start
/// 4. any messages sent between cells will come through Message
/// 5. when a cell determines that the organelle should stop, it can issue Stop
///     and the organelle will exit its event loop.
pub enum Protocol<M, R> where
    M: CellMessage,
    R: CellRole,
{
    /// initializes a cell with an effector to use
    Init(Effector<M, R>),
    /// add an input Handle with connection role
    AddInput(Handle, R),
    /// add an output Handle with connection role
    AddOutput(Handle, R),

    /// notifies cell that organelle has begun execution
    Start,

    /// internal use only - used to track source and destination of message
    Payload(Handle, Handle, M),

    /// updates the cell with a user-defined message from source cell Handle
    Message(Handle, M),

    /// tells the organelle to stop executing
    Stop,

    /// stop the organelle because of an error
    Err(Error),
}

impl<M, R> Protocol<M, R> where
    M: CellMessage,
    R: CellRole,
{
    fn convert_protocol<T, U>(msg: Protocol<T, U>) -> Self
        where
            M: From<T> + Into<T> + 'static,
            T: From<M> + Into<M> + 'static,

            R: From<U>
                + Into<U>
                + Debug
                + Copy
                + Clone
                + Hash
                + Eq
                + PartialEq
                + 'static,

            U: From<R>
                + Into<R>
                + Debug
                + Copy
                + Clone
                + Hash
                + Eq
                + PartialEq
                + 'static,
    {
        match msg {
            Protocol::Init(effector) => {
                let sender = effector.sender;

                let (tx, rx) = mpsc::channel(10);

                effector.reactor.spawn(
                    rx.for_each(move |msg| {
                        sender.clone().send(
                            Protocol::<T, U>::convert_protocol(msg)
                        )
                            .then(|_| Ok(()))
                    })
                );

                Protocol::Init(
                    Effector {
                        this_cell: effector.this_cell,
                        sender: tx,
                        reactor: effector.reactor,
                    }
                )
            },

            Protocol::AddInput(input, role) => Protocol::AddInput(
                input, role.into()
            ),
            Protocol::AddOutput(output, role) => Protocol::AddOutput(
                output, role.into()
            ),

            Protocol::Start => Protocol::Start,

            Protocol::Payload(src, dest, msg) => Protocol::Payload(
                src, dest, msg.into()
            ),
            Protocol::Message(src, msg) => Protocol::Message(
                src, msg.into()
            ),

            Protocol::Stop => Protocol::Stop,

            Protocol::Err(e) => Protocol::Err(e),
        }
    }
}

/// the effector is a cell's method of communicating between other cells
///
/// the effector can send a message to any destination, provided you have its
/// handle. it will route these messages asynchronously to their destination,
/// so communication can be tricky, however, this is truly the best way I've
/// found to compose efficient, scalable systems.
pub struct Effector<M, R> where
    M: CellMessage,
    R: CellRole
{
    this_cell:      Handle,
    sender:         mpsc::Sender<Protocol<M, R>>,
    reactor:        reactor::Handle,
}

impl<M, R> Clone for Effector<M, R> where
    M: CellMessage,
    R: CellRole
{
    fn clone(&self) -> Self {
        Self {
            this_cell: self.this_cell,
            sender: self.sender.clone(),
            reactor: self.reactor.clone(),
        }
    }
}

impl<M, R> Effector<M, R> where
    M: CellMessage,
    R: CellRole,
{
    /// get the Handle associated with the cell that owns this effector
    pub fn this_cell(&self) -> Handle {
        self.this_cell
    }

    /// send a message to dest cell
    pub fn send(&self, dest: Handle, msg: M) {
        self.send_organelle_message(
            Protocol::Payload(self.this_cell(), dest, msg)
        );
    }

    /// send a batch of messages in order to dest cell
    pub fn send_in_order(&self, dest: Handle, msgs: Vec<M>) {
        let src = self.this_cell();

        self.spawn(
            self.sender.clone()
                .send_all(
                    iter_ok(
                        msgs.into_iter()
                            .map(move |m| Protocol::Payload(src, dest, m))
                    )
                )
                .then(|_| Ok(()))
        );
    }

    /// stop the organelle
    pub fn stop(&self) {
        self.send_organelle_message(Protocol::Stop);
    }

    /// stop the organelle because of an error
    pub fn error(&self, e: Error) {
        self.send_organelle_message(Protocol::Err(e));
    }

    /// spawn a future on the reactor
    pub fn spawn<F>(&self, future: F) where
        F: Future<Item=(), Error=()> + 'static
    {
        self.reactor.spawn(future);
    }

    /// get a reactor handle
    pub fn reactor(&self) -> reactor::Handle {
        self.reactor.clone()
    }

    /// get a reactor remote
    pub fn remote(&self) -> reactor::Remote {
        self.reactor.remote().clone()
    }

    fn send_organelle_message(&self, msg: Protocol<M, R>) {
        self.spawn(self.sender.clone().send(msg).then(|_| Ok(())));
    }
}



trait Node<M, R> where
    M: CellMessage,
    R: CellRole,
{
    fn update(&mut self, msg: Protocol<M, R>) -> Result<()>;
}

struct CellWrapper<L, M, R>(Option<L>, PhantomData<M>, PhantomData<R>);

impl<L, M, R> CellWrapper<L, M, R> where
    L: Cell<Message=M, Role=R>,

    M: CellMessage,
    R: CellRole
{
    fn new(cell: L) -> Self {
        CellWrapper::<L, M, R>(
            Some(cell), PhantomData::default(), PhantomData::default()
        )
    }
}

impl<L, IM, OM, IR, OR> Node<OM, OR> for CellWrapper<L, IM, IR> where
    L: Cell<Message=IM, Role=IR>,

    IM: From<OM> + Into<OM> + 'static,
    OM: From<IM> + Into<IM> + 'static,

    IR: From<OR>
        + Into<OR>
        + Debug
        + Copy
        + Clone
        + Hash
        + Eq
        + PartialEq
        + 'static,

    OR: From<IR>
        + Into<IR>
        + Debug
        + Copy
        + Clone
        + Hash
        + Eq
        + PartialEq
        + 'static,
{
    fn update(&mut self, msg: Protocol<OM, OR>) -> Result<()> {
        if self.0.is_some() {
            let cell = mem::replace(&mut self.0, None)
                .unwrap()
                .update(Protocol::<IM, IR>::convert_protocol(msg))?
            ;

            self.0 = Some(cell);
        }

        Ok(())
    }
}
