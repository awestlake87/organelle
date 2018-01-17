use std;
use std::fmt::Debug;
use std::hash::Hash;
use std::intrinsics;

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
    /// error when a soma fails to update
    type Error: std::error::Error + Send + From<Error> + 'static;

    fn type_name() -> &'static str {
        unsafe { intrinsics::type_name::<Self>() }
    }

    /// apply any changes to the soma's state as a result of _msg
    fn update(
        self,
        _msg: Impulse<Self::Signal, Self::Synapse>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(self)
    }

    /// convert the soma into a future that can be run on an event loop
    fn into_future(
        self,
        reactor: reactor::Handle,
    ) -> Box<Future<Item = Result<()>, Error = Error>>
    where
        Self: 'static,
    {
        let (queue_tx, queue_rx) = mpsc::channel(100);

        let main_soma = Handle::new_v4();

        let sender = queue_tx.clone();

        // Rc to keep it alive, RefCell to mutate it in the event loop
        let mut node = SomaWrapper::new(self);

        let reactor_copy = reactor.clone();

        reactor.clone().spawn(
            queue_tx
                .clone()
                .send(Impulse::Init(
                    None,
                    Effector {
                        this_soma: main_soma,
                        sender: sender,
                        reactor: reactor,
                    },
                ))
                .and_then(|tx| {
                    tx.send(Impulse::Start).then(|result| match result {
                        Ok(_) => Ok(()),
                        Err(e) => panic!("unable to start main soma: {:?}", e),
                    })
                })
                .then(|result| match result {
                    Ok(_) => Ok(()),
                    Err(e) => panic!("unable to initialize main soma: {:?}", e),
                }),
        );

        let (tx, rx) = mpsc::channel::<Error>(1);

        let stream_future = queue_rx
            .take_while(|msg| match *msg {
                Impulse::Stop => Ok(false),
                _ => Ok(true),
            })
            .for_each(move |msg| {
                if let Err(e) = match msg {
                    Impulse::Init(parent, effector) => {
                        node.update(Impulse::Init(parent, effector))
                    },
                    Impulse::AddInput(input, role) => {
                        node.update(Impulse::AddInput(input, role))
                    },
                    Impulse::AddOutput(output, role) => {
                        node.update(Impulse::AddOutput(output, role))
                    },

                    Impulse::Start => node.update(Impulse::Start),

                    Impulse::Payload(src, dest, msg) => {
                        // messages should only be sent to our soma
                        assert_eq!(dest, main_soma);

                        node.update(Impulse::Signal(src, msg))
                    },
                    Impulse::Probe(dest) => {
                        // probes should only be send to our soma
                        assert_eq!(dest, main_soma);
                        node.update(Impulse::Probe(dest))
                    },

                    Impulse::Err(e) => Err(e),

                    _ => unreachable!(),
                } {
                    reactor_copy.spawn(tx.clone().send(e).then(|result| {
                        match result {
                            Ok(_) => Ok(()),
                            Err(e) => panic!("unable to send error: {:?}", e),
                        }
                    }));
                }

                Ok(())
            })
            .then(move |_| {
                // make sure the channel stays open
                let _keep_alive = queue_tx;

                Ok(())
            });

        Box::new(
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
        )
    }

    /// spin up an event loop and run soma
    fn run(self) -> Result<()>
    where
        Self: 'static,
    {
        let mut core = reactor::Core::new()?;

        let reactor = core.handle();

        core.run(self.into_future(reactor))?
    }
}
