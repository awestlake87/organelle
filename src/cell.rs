
use std::fmt::Debug;
use std::hash::Hash;

use futures::prelude::*;
use futures::sync::{ mpsc };
use tokio_core::reactor;

use super::{
    Result, Error, ErrorKind, Protocol, Handle, Effector, CellWrapper, Node
};

/// defines the collection of traits necessary to act as a cell message
pub trait CellMessage: 'static { }

impl<T> CellMessage for T where T: 'static { }

/// defines the collection of traits necessary to act as a cell role
pub trait CellRole: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static { }

impl<T> CellRole for T where
    T: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static
{ }

/// defines an interface for a cell of any type
///
/// generic across the user-defined message to be passed between cells and the
/// user-defined roles for connections
pub trait Cell: Sized {
    /// user-defined message to be passed between cells
    type Message: CellMessage;
    /// user-defined roles for connections
    type Role: CellRole;

    /// apply any changes to the cell's state as a result of _msg
    fn update(self, _msg: Protocol<Self::Message, Self::Role>)
        -> Result<Self>
    {
        Ok(self)
    }

    /// spin up an event loop and run cell
    fn run(self) -> Result<()> {
        let (queue_tx, queue_rx) = mpsc::channel(100);
        let mut core = reactor::Core::new()?;

        let main_cell = Handle::new_v4();
        let reactor = core.handle();

        let sender = queue_tx.clone();

        // Rc to keep it alive, RefCell to mutate it in the event loop
        let mut node = CellWrapper::new(self);

        reactor.clone().spawn(
            queue_tx.clone()
                .send(
                    Protocol::Init(
                        Effector {
                            this_cell: main_cell,
                            sender: sender,
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
                        // messages should only be sent to our main cell
                        assert_eq!(dest, main_cell);

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
                .map_err(|_| -> Error {
                    ErrorKind::Msg("select error".into()).into()
                })
        )?;

        result
    }
}