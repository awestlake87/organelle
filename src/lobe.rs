
use std::rc::Rc;

use futures::prelude::*;
use futures::sync::{ mpsc };
use tokio_core::reactor;

use super::{
    Result, Error, ErrorKind, Protocol, Handle, Effector, LobeWrapper, Node
};

/// defines an interface for a lobe of any type
///
/// generic across the user-defined message to be passed between lobes and the
/// user-defined roles for connections
pub trait Lobe: Sized {
    /// user-defined message to be passed between lobes
    type Message: 'static;
    /// user-defined roles for connections
    type Role: Copy + Clone + Eq + PartialEq + 'static;

    /// apply any changes to the lobe's state as a result of _msg
    fn update(self, _msg: Protocol<Self::Message, Self::Role>)
        -> Result<Self>
    {
        Ok(self)
    }
}

/// spin up an event loop and run the provided lobe
pub fn run<T, M, R>(lobe: T) -> Result<()> where
    T: Lobe<Message=M, Role=R>,
    M: 'static,
    R: Copy + Clone + Eq + PartialEq + 'static,
{
    let (queue_tx, queue_rx) = mpsc::channel(100);
    let mut core = reactor::Core::new()?;

    let handle = Handle::new_v4();
    let reactor = core.handle();

    let sender_tx = queue_tx.clone();
    let sender: Rc<Fn(&reactor::Handle, Protocol<M, R>)> = Rc::from(
        move |r: &reactor::Handle, msg| r.spawn(
             sender_tx.clone()
                .send(msg)
                .then(|_| Ok(()))
        )
    );

    // Rc to keep it alive, RefCell to mutate it in the event loop
    let mut node = LobeWrapper::new(lobe);

    reactor.clone().spawn(
        queue_tx.clone()
            .send(
                Protocol::Init(
                    Effector {
                        handle: handle,
                        sender: Rc::clone(&sender),
                        reactor: reactor,
                    }
                )
            )
            .and_then(
                |tx| tx.send(Protocol::Start)
                    .then(|_| Ok(()))
            )
            .then(|_| Ok(()))

    );

    let (tx, rx) = mpsc::channel::<Error>(1);
    let reactor = core.handle();

    let stream_future = queue_rx.take_while(
        |msg| match *msg {
            Protocol::Stop => Ok(false),
            _ => Ok(true)
        }
    ).for_each(
        move |msg| {
            if let Err(e) = match msg {
                Protocol::Init(effector) => node.update(
                    Protocol::Init(effector)
                ),
                Protocol::AddInput(input, role) => node.update(
                    Protocol::AddInput(input, role)
                ),
                Protocol::AddOutput(output, role) => node.update(
                    Protocol::AddOutput(output, role)
                ),

                Protocol::Start => node.update(Protocol::Start),

                Protocol::Payload(src, dest, msg) => {
                    // messages should only be sent to our main lobe
                    assert_eq!(dest, handle);

                    node.update(Protocol::Message(src, msg))
                },

                Protocol::Err(e) => Err(e),

                _ => unreachable!(),
            } {
                reactor.spawn(
                    tx.clone().send(e).then(|_| Ok(()))
                );
            }

            Ok(())
        }
    );

    let result = core.run(
        stream_future
            .map(|_| Ok(()))
            .select(
                rx.into_future()
                    .map(|(item, _)| Err(item.unwrap()))
                    .map_err(|_| ())
            )
            .map(|(result, _)| result)
            .map_err(
                |_| -> Error { ErrorKind::Msg("select error".into()).into() }
            )
    )?;

    result
}
