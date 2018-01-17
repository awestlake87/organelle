#![warn(missing_docs)]
#![feature(core_intrinsics)]

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
mod probe;

pub use axon::{Axon, Dendrite, Neuron, Sheath};
pub use organelle::Organelle;
pub use probe::{ProbeData, ProbeSignal, ProbeSoma, ProbeSynapse};
pub use soma::{Signal, Soma, Synapse};

use std::mem;

use futures::prelude::*;
use futures::stream::iter_ok;
use futures::sync;
use futures::unsync::mpsc;
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
/// 3. when the organelle is ready to begin execution, every soma is updated
/// with     Start
/// 4. any messages sent between somas will come through Signal
/// 5. when a soma determines that the organelle should stop, it can issue Stop
///     and the organelle will exit its event loop.
pub enum Impulse<S: Signal, Y: Synapse> {
    /// initializes a soma with the parent handle and an effector to use
    Init(Option<Handle>, Effector<S, Y>),
    /// add an input Handle with connection role
    AddInput(Handle, Y),
    /// add an output Handle with connection role
    AddOutput(Handle, Y),

    /// notifies soma that organelle has begun execution
    Start,

    /// internal use only - used to track source and destination of message
    Payload(Handle, Handle, S),

    /// updates the soma with a user-defined message from source soma
    /// Handle
    Signal(Handle, S),

    /// tells the organelle to stop executing
    Stop,

    /// collect information about the organelle system
    Probe(Handle),

    /// stop the organelle because of an error
    Err(Error),
}

impl<S: Signal, Y: Synapse> Impulse<S, Y> {
    fn convert_protocol<T, U>(msg: Impulse<T, U>) -> Self
    where
        S: From<T> + Into<T>,
        T: From<S> + Into<S> + Signal,

        Y: From<U> + Into<U>,
        U: From<Y> + Into<Y> + Synapse,
    {
        match msg {
            Impulse::Init(parent, effector) => {
                let sender = effector.sender;

                let (tx, rx) = mpsc::channel(10);

                effector.reactor.spawn(rx.for_each(move |msg| {
                    sender
                        .clone()
                        .send(Impulse::<T, U>::convert_protocol(msg))
                        .then(|_| Ok(()))
                }));

                Impulse::Init(
                    parent,
                    Effector {
                        this_soma: effector.this_soma,
                        sender: tx,
                        reactor: effector.reactor,
                    },
                )
            },

            Impulse::AddInput(input, role) => {
                Impulse::AddInput(input, role.into())
            },
            Impulse::AddOutput(output, role) => {
                Impulse::AddOutput(output, role.into())
            },

            Impulse::Start => Impulse::Start,

            Impulse::Payload(src, dest, msg) => {
                Impulse::Payload(src, dest, msg.into())
            },
            Impulse::Signal(src, msg) => Impulse::Signal(src, msg.into()),

            Impulse::Stop => Impulse::Stop,
            Impulse::Probe(dest) => Impulse::Probe(dest),

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
pub struct Effector<S: Signal, Y: Synapse> {
    this_soma: Handle,
    sender: mpsc::Sender<Impulse<S, Y>>,
    reactor: reactor::Handle,
}

impl<S: Signal, Y: Synapse> Clone for Effector<S, Y> {
    fn clone(&self) -> Self {
        Self {
            this_soma: self.this_soma,
            sender: self.sender.clone(),
            reactor: self.reactor.clone(),
        }
    }
}

impl<S: Signal, Y: Synapse> Effector<S, Y> {
    /// get the Handle associated with the soma that owns this effector
    pub fn this_soma(&self) -> Handle {
        self.this_soma
    }

    /// send a message to dest soma
    pub fn send(&self, dest: Handle, msg: S) {
        self.send_organelle_message(Impulse::Payload(
            self.this_soma(),
            dest,
            msg,
        ));
    }

    /// send a batch of messages in order to dest soma
    pub fn send_in_order(&self, dest: Handle, msgs: Vec<S>) {
        let src = self.this_soma();

        self.spawn(
            self.sender
                .clone()
                .send_all(iter_ok(
                    msgs.into_iter()
                        .map(move |m| Impulse::Payload(src, dest, m)),
                ))
                .then(|_| Ok(())),
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

    /// probe for information about the organelle
    pub fn probe(&self, dest: Handle) {
        self.send_organelle_message(Impulse::Probe(dest));
    }

    /// spawn a future on the reactor
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Item = (), Error = ()> + 'static,
    {
        self.reactor.spawn(future);
    }

    /// get a reactor handle
    pub fn reactor(&self) -> reactor::Handle {
        self.reactor.clone()
    }

    /// get a remote effector for use across threads
    pub fn remote(&self) -> RemoteEffector<S>
    where
        S: Send,
    {
        let (tx, rx) = sync::mpsc::channel(10);

        let sender = self.sender.clone();

        self.spawn(
            rx.map(|imp| match imp {
                RemoteImpulse::Payload(src, dest, signal) => {
                    Impulse::Payload(src, dest, signal)
                },
                RemoteImpulse::Stop => Impulse::Stop,
                RemoteImpulse::Err(e) => Impulse::Err(e),
            }).for_each(move |imp| sender.clone().send(imp).then(|_| Ok(())))
                .then(|_| Ok(())),
        );

        RemoteEffector {
            this_soma: self.this_soma,
            sender: tx,
            reactor: self.reactor().remote().clone(),
        }
    }

    fn send_organelle_message(&self, msg: Impulse<S, Y>) {
        self.spawn(self.sender.clone().send(msg).then(|_| Ok(())));
    }
}

enum RemoteImpulse<S: Signal + Send> {
    Payload(Handle, Handle, S),
    Stop,
    Err(Error),
}

/// the remote effector is an effector meant to be used between threads
pub struct RemoteEffector<S: Signal + Send> {
    this_soma: Handle,
    sender: sync::mpsc::Sender<RemoteImpulse<S>>,
    reactor: reactor::Remote,
}

impl<S: Signal + Send> Clone for RemoteEffector<S> {
    fn clone(&self) -> Self {
        Self {
            this_soma: self.this_soma,
            sender: self.sender.clone(),
            reactor: self.reactor.clone(),
        }
    }
}

impl<S: Signal + Send> RemoteEffector<S> {
    /// get the Handle associated with the soma that owns this effector
    pub fn this_soma(&self) -> Handle {
        self.this_soma
    }

    /// send a message to dest soma
    pub fn send(&self, dest: Handle, msg: S) {
        self.send_organelle_message(RemoteImpulse::Payload(
            self.this_soma(),
            dest,
            msg,
        ));
    }

    /// send a batch of messages in order to dest soma
    pub fn send_in_order(&self, dest: Handle, msgs: Vec<S>) {
        let src = self.this_soma();

        self.spawn(
            self.sender
                .clone()
                .send_all(iter_ok(
                    msgs.into_iter()
                        .map(move |m| RemoteImpulse::Payload(src, dest, m)),
                ))
                .then(|_| Ok(())),
        );
    }

    /// stop the organelle
    pub fn stop(&self) {
        self.send_organelle_message(RemoteImpulse::Stop);
    }

    /// stop the organelle because of an error
    pub fn error(&self, e: Error) {
        self.send_organelle_message(RemoteImpulse::Err(e));
    }

    /// spawn a future on the reactor
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Item = (), Error = ()> + Send + 'static,
    {
        self.reactor.spawn(move |_| future);
    }

    /// get a reactor handle
    pub fn reactor(&self) -> reactor::Remote {
        self.reactor.clone()
    }

    fn send_organelle_message(&self, msg: RemoteImpulse<S>) {
        self.spawn(self.sender.clone().send(msg).then(|_| Ok(())));
    }
}

trait Node<S: Signal, Y: Synapse> {
    fn type_name(&self) -> &'static str;
    fn update(&mut self, msg: Impulse<S, Y>) -> Result<()>;
}

struct SomaWrapper<T: Soma>(Option<T>);

impl<T: Soma> SomaWrapper<T> {
    fn new(soma: T) -> Self {
        SomaWrapper::<T>(Some(soma))
    }
}

impl<T: Soma, OS, OY> Node<OS, OY> for SomaWrapper<T>
where
    T::Signal: From<OS> + Into<OS> + Signal,
    OS: From<T::Signal> + Into<T::Signal> + Signal,

    T::Synapse: From<OY> + Into<OY> + Synapse,
    OY: From<T::Synapse> + Into<T::Synapse> + Synapse,
{
    fn type_name(&self) -> &'static str {
        T::type_name()
    }
    fn update(&mut self, msg: Impulse<OS, OY>) -> Result<()> {
        if self.0.is_some() {
            match mem::replace(&mut self.0, None)
                .unwrap()
                .update(Impulse::<T::Signal, T::Synapse>::convert_protocol(msg))
            {
                Ok(soma) => self.0 = Some(soma),
                Err(e) => {
                    return Err(Error::with_chain(e, ErrorKind::SomaError))
                },
            }
        }

        Ok(())
    }
}
