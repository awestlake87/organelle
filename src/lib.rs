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
pub enum Protocol<M: 'static, C: Copy + Clone + Eq + PartialEq + 'static> {
    /// initializes a lobe with an effector to use
    Init(Effector<M, C>),
    /// add an input Handle with connection constraint
    AddInput(Handle, C),
    /// add an output Handle with connection constraint
    AddOutput(Handle, C),

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

impl<M, C> Protocol<M, C> where
    M: 'static,
    C: Copy + Clone + Eq + PartialEq + 'static,
{
    fn convert_protocol<T, U>(msg: Protocol<T, U>) -> Self
        where
            M: From<T> + Into<T> + 'static,
            T: From<M> + Into<M> + 'static,

            C: From<U> + Into<U> + Copy + Clone + Eq + PartialEq + 'static,
            U: From<C> + Into<C> + Copy + Clone + Eq + PartialEq + 'static,
    {
        match msg {
            Protocol::Init(effector) => {
                let sender = effector.sender;

                Protocol::Init(
                    Effector {
                        handle: effector.handle,
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

            Protocol::AddInput(input, constraint) => Protocol::AddInput(
                input, constraint.into()
            ),
            Protocol::AddOutput(output, constraint) => Protocol::AddOutput(
                output, constraint.into()
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
pub struct Effector<M: 'static, C: Copy + Clone + Eq + PartialEq + 'static> {
    handle:     Handle,
    sender:     Rc<Fn(&reactor::Handle, Protocol<M, C>)>,
    reactor:    reactor::Handle,
}

impl<M, C> Clone for Effector<M, C> where
    M: 'static,
    C: Copy + Clone + Eq + PartialEq + 'static,
{
    fn clone(&self) -> Self {
        Self {
            handle: self.handle,
            sender: Rc::clone(&self.sender),
            reactor: self.reactor.clone(),
        }
    }
}

impl<M, C> Effector<M, C> where
    M: 'static,
    C: Copy + Clone + Eq + PartialEq + 'static,
{
    /// get the Handle associated with the lobe that owns this effector
    pub fn handle(&self) -> Handle {
        self.handle
    }

    /// send a message to dest lobe
    pub fn send(&self, dest: Handle, msg: M) {
        self.send_cortex_message(
            Protocol::Payload(self.handle(), dest, msg)
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

    fn send_cortex_message(&self, msg: Protocol<M, C>) {
        (*self.sender)(&self.reactor, msg);
    }
}

trait Node<M: 'static, C: Copy + Clone + Eq + PartialEq + 'static> {
    fn update(&mut self, msg: Protocol<M, C>) -> Result<()>;
}

struct LobeWrapper<L, M, C>(Option<L>, PhantomData<M>, PhantomData<C>);

impl<L, M, C> LobeWrapper<L, M, C> where
    L: Lobe<Message=M, Constraint=C>,

    M: 'static,
    C: Copy + Clone + Eq + PartialEq + 'static
{
    fn new(lobe: L) -> Self {
        LobeWrapper::<L, M, C>(
            Some(lobe), PhantomData::default(), PhantomData::default()
        )
    }
}

impl<L, IM, OM, IC, OC> Node<OM, OC> for LobeWrapper<L, IM, IC> where
    L: Lobe<Message=IM, Constraint=IC>,

    IM: From<OM> + Into<OM> + 'static,
    OM: From<IM> + Into<IM> + 'static,

    IC: From<OC> + Into<OC> + Copy + Clone + Eq + PartialEq + 'static,
    OC: From<IC> + Into<IC> + Copy + Clone + Eq + PartialEq + 'static,
{
    fn update(&mut self, msg: Protocol<OM, OC>) -> Result<()> {
        if self.0.is_some() {
            let lobe = mem::replace(&mut self.0, None)
                .unwrap()
                .update(Protocol::<IM, IC>::convert_protocol(msg))?
            ;

            self.0 = Some(lobe);
        }

        Ok(())
    }
}
