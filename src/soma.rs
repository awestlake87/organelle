use std::fmt::Debug;
use std::hash::Hash;

use futures::prelude::*;
use futures::unsync::mpsc;
use tokio_core::reactor;

use super::{
    Effector,
    Error,
    ErrorKind,
    Handle,
    Impulse,
    Node,
    Result,
    SomaWrapper,
};

/// defines the collection of traits necessary to act as a soma message
pub trait Signal: 'static {}

impl<T> Signal for T
where
    T: 'static,
{
}

/// defines the collection of traits necessary to act as a soma role
pub trait Synapse: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static {}

impl<T> Synapse for T
where
    T: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static,
{
}

/// defines an interface for a soma of any type
///
/// generic across the user-defined message to be passed between somas and the
/// user-defined roles for connections
pub trait Soma: Sized {
    /// user-defined message to be passed between somas
    type Signal: Signal;
    /// user-defined roles for connections
    type Synapse: Synapse;

    /// apply any changes to the soma's state as a result of _msg
    fn update(
        self,
        _msg: Impulse<Self::Signal, Self::Synapse>,
    ) -> Result<Self> {
        Ok(self)
    }

    /// spin up an event loop and run soma
    fn run(self) -> Result<()> {
        let (queue_tx, queue_rx) = mpsc::channel(100);
        let mut core = reactor::Core::new()?;

        let main_soma = Handle::new_v4();
        let reactor = core.handle();

        let sender = queue_tx.clone();

        // Rc to keep it alive, RefCell to mutate it in the event loop
        let mut node = SomaWrapper::new(self);

        reactor.clone().spawn(
            queue_tx
                .clone()
                .send(Impulse::Init(Effector {
                    this_soma: main_soma,
                    sender: sender,
                    reactor: reactor,
                }))
                .and_then(|tx| tx.send(Impulse::Start).then(|_| Ok(())))
                .then(|_| Ok(())),
        );

        let (tx, rx) = mpsc::channel::<Error>(1);
        let reactor = core.handle();

        let stream_future = queue_rx
            .take_while(|msg| match *msg {
                Impulse::Stop => Ok(false),
                _ => Ok(true),
            })
            .for_each(move |msg| {
                if let Err(e) = match msg {
                    Impulse::Init(effector) => {
                        node.update(Impulse::Init(effector))
                    },
                    Impulse::AddInput(input, role) => {
                        node.update(Impulse::AddInput(input, role))
                    },
                    Impulse::AddOutput(output, role) => {
                        node.update(Impulse::AddOutput(output, role))
                    },

                    Impulse::Start => node.update(Impulse::Start),

                    Impulse::Payload(src, dest, msg) => {
                        // messages should only be sent to our main soma
                        assert_eq!(dest, main_soma);

                        node.update(Impulse::Signal(src, msg))
                    },

                    Impulse::Err(e) => Err(e),

                    _ => unreachable!(),
                } {
                    reactor.spawn(tx.clone().send(e).then(|_| Ok(())));
                }

                Ok(())
            });

        let result = core.run(
            stream_future
                .map(|_| Ok(()))
                .select(
                    rx.into_future()
                        .map(|(item, _)| Err(item.unwrap()))
                        .map_err(|_| ()),
                )
                .map(|(result, _)| result)
                .map_err(|_| -> Error {
                    ErrorKind::Msg("select error".into()).into()
                }),
        )?;

        result
    }
}
