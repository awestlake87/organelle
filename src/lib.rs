#![warn(missing_docs)]

//! Organelle - reactive architecture for emergent AI systems

#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate tokio;
extern crate tokio_core;
extern crate uuid;

mod organelle;
mod soma;
mod axon;

pub use organelle::{ Organelle };
pub use soma::{ Soma, SomaSignal, SomaSynapse };
pub use axon::{ Axon, Dendrite, Neuron, Sheath };

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
        /// a soma returned an error when called into
        SomaError {
            description("an error occurred while calling into a soma"),
            display("an error occurred while calling into a soma")
        }
    }
}

/// handle to a soma within the organelle
pub type Handle = Uuid;

/// a set of protocol messages to be relayed throughout the network
///
/// wraps a user-defined message within the organelle protocol.
/// 1. a soma is always updated with Init first.
/// 2. as the organelle is built, the soma will be updated with any inputs or
///     outputs specified using AddInput and AddOutput.
/// 3. when the organelle is ready to begin execution, every soma is updated with
///     Start
/// 4. any messages sent between somas will come through Signal
/// 5. when a soma determines that the organelle should stop, it can issue Stop
///     and the organelle will exit its event loop.
pub enum Impulse<S, Y> where
    S: SomaSignal,
    Y: SomaSynapse,
{
    /// initializes a soma with an effector to use
    Init(Effector<S, Y>),
    /// add an input Handle with connection role
    AddInput(Handle, Y),
    /// add an output Handle with connection role
    AddOutput(Handle, Y),

    /// notifies soma that organelle has begun execution
    Start,

    /// internal use only - used to track source and destination of message
    Payload(Handle, Handle, S),

    /// updates the soma with a user-defined message from source soma Handle
    Signal(Handle, S),

    /// tells the organelle to stop executing
    Stop,

    /// stop the organelle because of an error
    Err(Error),
}

impl<S, Y> Impulse<S, Y> where
    S: SomaSignal,
    Y: SomaSynapse,
{
    fn convert_protocol<T, U>(msg: Impulse<T, U>) -> Self
        where
            S: From<T> + Into<T> + 'static,
            T: From<S> + Into<S> + 'static,

            Y: From<U>
                + Into<U>
                + Debug
                + Copy
                + Clone
                + Hash
                + Eq
                + PartialEq
                + 'static,

            U: From<Y>
                + Into<Y>
                + Debug
                + Copy
                + Clone
                + Hash
                + Eq
                + PartialEq
                + 'static,
    {
        match msg {
            Impulse::Init(effector) => {
                let sender = effector.sender;

                let (tx, rx) = mpsc::channel(10);

                effector.reactor.spawn(
                    rx.for_each(move |msg| {
                        sender.clone().send(
                            Impulse::<T, U>::convert_protocol(msg)
                        )
                            .then(|_| Ok(()))
                    })
                );

                Impulse::Init(
                    Effector {
                        this_soma: effector.this_soma,
                        sender: tx,
                        reactor: effector.reactor,
                    }
                )
            },

            Impulse::AddInput(input, role) => Impulse::AddInput(
                input, role.into()
            ),
            Impulse::AddOutput(output, role) => Impulse::AddOutput(
                output, role.into()
            ),

            Impulse::Start => Impulse::Start,

            Impulse::Payload(src, dest, msg) => Impulse::Payload(
                src, dest, msg.into()
            ),
            Impulse::Signal(src, msg) => Impulse::Signal(
                src, msg.into()
            ),

            Impulse::Stop => Impulse::Stop,

            Impulse::Err(e) => Impulse::Err(e),
        }
    }
}

/// the effector is a soma's method of communicating between other somas
///
/// the effector can send a message to any destination, provided you have its
/// handle. it will route these messages asynchronously to their destination,
/// so communication can be tricky, however, this is truly the best way I've
/// found to compose efficient, scalable systems.
pub struct Effector<S, Y> where
    S: SomaSignal,
    Y: SomaSynapse
{
    this_soma:      Handle,
    sender:         mpsc::Sender<Impulse<S, Y>>,
    reactor:        reactor::Handle,
}

impl<S, Y> Clone for Effector<S, Y> where
    S: SomaSignal,
    Y: SomaSynapse
{
    fn clone(&self) -> Self {
        Self {
            this_soma: self.this_soma,
            sender: self.sender.clone(),
            reactor: self.reactor.clone(),
        }
    }
}

impl<S, Y> Effector<S, Y> where
    S: SomaSignal,
    Y: SomaSynapse,
{
    /// get the Handle associated with the soma that owns this effector
    pub fn this_soma(&self) -> Handle {
        self.this_soma
    }

    /// send a message to dest soma
    pub fn send(&self, dest: Handle, msg: S) {
        self.send_organelle_message(
            Impulse::Payload(self.this_soma(), dest, msg)
        );
    }

    /// send a batch of messages in order to dest soma
    pub fn send_in_order(&self, dest: Handle, msgs: Vec<S>) {
        let src = self.this_soma();

        self.spawn(
            self.sender.clone()
                .send_all(
                    iter_ok(
                        msgs.into_iter()
                            .map(move |m| Impulse::Payload(src, dest, m))
                    )
                )
                .then(|_| Ok(()))
        );
    }

    /// stop the organelle
    pub fn stop(&self) {
        self.send_organelle_message(Impulse::Stop);
    }

    /// stop the organelle because of an error
    pub fn error(&self, e: Error) {
        self.send_organelle_message(Impulse::Err(e));
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

    fn send_organelle_message(&self, msg: Impulse<S, Y>) {
        self.spawn(self.sender.clone().send(msg).then(|_| Ok(())));
    }
}



trait Node<S, Y> where
    S: SomaSignal,
    Y: SomaSynapse,
{
    fn update(&mut self, msg: Impulse<S, Y>) -> Result<()>;
}

struct SomaWrapper<L, S, Y>(Option<L>, PhantomData<S>, PhantomData<Y>);

impl<L, S, Y> SomaWrapper<L, S, Y> where
    L: Soma<Signal=S, Synapse=Y>,

    S: SomaSignal,
    Y: SomaSynapse
{
    fn new(soma: L) -> Self {
        SomaWrapper::<L, S, Y>(
            Some(soma), PhantomData::default(), PhantomData::default()
        )
    }
}

impl<L, IM, OM, IR, OR> Node<OM, OR> for SomaWrapper<L, IM, IR> where
    L: Soma<Signal=IM, Synapse=IR>,

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
    fn update(&mut self, msg: Impulse<OM, OR>) -> Result<()> {
        if self.0.is_some() {
            let soma = mem::replace(&mut self.0, None)
                .unwrap()
                .update(Impulse::<IM, IR>::convert_protocol(msg))?
            ;

            self.0 = Some(soma);
        }

        Ok(())
    }
}
