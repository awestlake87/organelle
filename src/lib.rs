#![warn(missing_docs)]

//! Cortical - general purpose reactive lobe networks

#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate tokio;
extern crate tokio_core;
extern crate uuid;

mod cortex;
mod lobe;

pub use cortex::{ Cortex };
pub use lobe::{ Lobe, run };

use std::marker::PhantomData;
use std::mem;
use std::rc::Rc;

use futures::prelude::*;
use tokio_core::reactor;
use uuid::Uuid;

/// cortical error
error_chain! {
    foreign_links {
        Io(std::io::Error) #[doc = "glue for io::Error"];
        Canceled(futures::Canceled) #[doc = "glue for futures::Canceled"];
    }
    errors {
        /// a lobe returned an error when called into
        LobeError {
            description("an error occurred while calling into a lobe"),
            display("an error occurred while calling into a lobe")
        }
    }
}

/// handle to a lobe within the cortex
pub type Handle = Uuid;

/// a set of protocol messages to be relayed throughout the network
///
/// wraps a user-defined message within the cortex protocol.
/// 1. a lobe is always updated with Init first.
/// 2. as the cortex is built, the lobe will be updated with any inputs or
///     outputs specified using AddInput and AddOutput.
/// 3. when the cortex is ready to begin execution, every lobe is updated with
///     Start
/// 4. any messages sent between lobes will come through Message
/// 5. when a lobe determines that the cortex should stop, it can issue Stop
///     and the cortex will exit its event loop.
pub enum Protocol<M: 'static, R: Copy + Clone + Eq + PartialEq + 'static> {
    /// initializes a lobe with an effector to use
    Init(Effector<M, R>),
    /// add an input Handle with connection role
    AddInput(Handle, R),
    /// add an output Handle with connection role
    AddOutput(Handle, R),

    /// notifies lobe that cortex has begun execution
    Start,

    /// internal use only - used to track source and destination of message
    Payload(Handle, Handle, M),

    /// updates the lobe with a user-defined message from source lobe Handle
    Message(Handle, M),

    /// tells the cortex to stop executing
    Stop,

    /// stop the cortex because of an error
    Err(Error),
}

impl<M, R> Protocol<M, R> where
    M: 'static,
    R: Copy + Clone + Eq + PartialEq + 'static,
{
    fn convert_protocol<T, U>(msg: Protocol<T, U>) -> Self
        where
            M: From<T> + Into<T> + 'static,
            T: From<M> + Into<M> + 'static,

            R: From<U> + Into<U> + Copy + Clone + Eq + PartialEq + 'static,
            U: From<R> + Into<R> + Copy + Clone + Eq + PartialEq + 'static,
    {
        match msg {
            Protocol::Init(effector) => {
                let sender = effector.sender;

                Protocol::Init(
                    Effector {
                        this_lobe: effector.this_lobe,
                        sender: Rc::from(
                            move |effector: &reactor::Handle, msg| sender(
                                effector,
                                Protocol::<T, U>::convert_protocol(msg)
                            )
                        ),
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

/// the effector is a lobe's method of communicating between other lobes
///
/// the effector can send a message to any destination, provided you have its
/// handle. it will route these messages asynchronously to their destination,
/// so communication can be tricky, however, this is truly the best way I've
/// found to compose efficient, scalable systems.
pub struct Effector<M: 'static, R: Copy + Clone + Eq + PartialEq + 'static> {
    this_lobe:      Handle,
    sender:         Rc<Fn(&reactor::Handle, Protocol<M, R>)>,
    reactor:        reactor::Handle,
}

impl<M, R> Clone for Effector<M, R> where
    M: 'static,
    R: Copy + Clone + Eq + PartialEq + 'static,
{
    fn clone(&self) -> Self {
        Self {
            this_lobe: self.this_lobe,
            sender: Rc::clone(&self.sender),
            reactor: self.reactor.clone(),
        }
    }
}

impl<M, R> Effector<M, R> where
    M: 'static,
    R: Copy + Clone + Eq + PartialEq + 'static,
{
    /// get the Handle associated with the lobe that owns this effector
    pub fn this_lobe(&self) -> Handle {
        self.this_lobe
    }

    /// send a message to dest lobe
    pub fn send(&self, dest: Handle, msg: M) {
        self.send_cortex_message(
            Protocol::Payload(self.this_lobe(), dest, msg)
        );
    }

    /// stop the cortex
    pub fn stop(&self) {
        self.send_cortex_message(Protocol::Stop);
    }

    /// stop the cortex because of an error
    pub fn error(&self, e: Error) {
        self.send_cortex_message(Protocol::Err(e));
    }

    /// spawn a future on the reactor
    pub fn spawn<F>(&self, future: F) where
        F: Future<Item=(), Error=()> + 'static
    {
        self.reactor.spawn(future);
    }

    /// get a reactor remote
    pub fn remote(&self) -> reactor::Remote {
        self.reactor.remote().clone()
    }

    fn send_cortex_message(&self, msg: Protocol<M, R>) {
        (*self.sender)(&self.reactor, msg);
    }
}

trait Node<M: 'static, R: Copy + Clone + Eq + PartialEq + 'static> {
    fn update(&mut self, msg: Protocol<M, R>) -> Result<()>;
}

struct LobeWrapper<L, M, R>(Option<L>, PhantomData<M>, PhantomData<R>);

impl<L, M, R> LobeWrapper<L, M, R> where
    L: Lobe<Message=M, Role=R>,

    M: 'static,
    R: Copy + Clone + Eq + PartialEq + 'static
{
    fn new(lobe: L) -> Self {
        LobeWrapper::<L, M, R>(
            Some(lobe), PhantomData::default(), PhantomData::default()
        )
    }
}

impl<L, IM, OM, IR, OR> Node<OM, OR> for LobeWrapper<L, IM, IR> where
    L: Lobe<Message=IM, Role=IR>,

    IM: From<OM> + Into<OM> + 'static,
    OM: From<IM> + Into<IM> + 'static,

    IR: From<OR> + Into<OR> + Copy + Clone + Eq + PartialEq + 'static,
    OR: From<IR> + Into<IR> + Copy + Clone + Eq + PartialEq + 'static,
{
    fn update(&mut self, msg: Protocol<OM, OR>) -> Result<()> {
        if self.0.is_some() {
            let lobe = mem::replace(&mut self.0, None)
                .unwrap()
                .update(Protocol::<IM, IR>::convert_protocol(msg))?
            ;

            self.0 = Some(lobe);
        }

        Ok(())
    }
}
