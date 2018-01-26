use std;
use std::fmt::Debug;
use std::hash::Hash;
use std::intrinsics;

use futures::prelude::*;
use futures::unsync::{mpsc, oneshot};
use tokio_core::reactor;
use uuid::Uuid;

use super::{Error, Result};
use probe::{SomaData, SynapseData};

/// trait alias to express requirements of a Synapse type
pub trait Synapse: Debug + Copy + Clone + Hash + PartialEq + Eq {
    /// terminals are the senders or outputs in a connection between somas
    type Terminal: Debug;
    /// dendrites are the receivers or inputs in a connection between somas
    type Dendrite: Debug;

    fn data() -> SynapseData {
        SynapseData(unsafe { intrinsics::type_name::<Self>().to_string() })
    }

    /// form a synapse for this synapse into a terminal and dendrite
    fn synapse(self) -> (Self::Terminal, Self::Dendrite);
}

/// a group of control signals passed between somas
#[derive(Debug)]
pub enum Impulse<R: Synapse> {
    /// add a dendrite for input to the soma
    ///
    /// you should always expect to handle this impulse if the soma has any
    /// inputs. if your soma has inputs, it is best to wrap it with an Axon
    /// which can be used for validation purposes.
    AddDendrite(Uuid, R, R::Dendrite),
    /// add a terminal for output to the soma
    ///
    /// you should always expect to handle this impulse if the soma has any
    /// outputs. if your soma has outputs, it is best to wrap it with an Axon
    /// which can be used for validation purposes.
    AddTerminal(Uuid, R, R::Terminal),
    /// notify the soma that it has received all of its inputs and outputs
    ///
    /// you should always expect to handle this impulse because it will be
    /// passed to each soma regardless of configuration
    Start(Uuid, mpsc::Sender<Impulse<R>>, reactor::Handle),
    /// stop the event loop and exit gracefully
    ///
    /// you should not expect to handle this impulse at any time, it is handled
    /// for you by the event loop
    Stop,
    /// terminate the event loop with an error
    ///
    /// this impulse will automatically be triggered if a soma update resolves
    /// with an error.
    ///
    /// you should not expect to handle this impulse at any time, it is handled
    /// for you by the event loop
    Error(Error),
    Probe(oneshot::Sender<SomaData>),
}

impl<R> Impulse<R>
where
    R: Synapse,
{
    /// convert from another type of impulse
    pub fn convert_from<T>(imp: Impulse<T>) -> Self
    where
        T: Synapse + Into<R>,
        T::Dendrite: Into<R::Dendrite>,
        T::Terminal: Into<R::Terminal>,
    {
        match imp {
            Impulse::AddDendrite(uuid, synapse, dendrite) => {
                Impulse::AddDendrite(uuid, synapse.into(), dendrite.into())
            },
            Impulse::AddTerminal(uuid, synapse, terminal) => {
                Impulse::AddTerminal(uuid, synapse.into(), terminal.into())
            },
            Impulse::Stop => Impulse::Stop,
            Impulse::Error(e) => Impulse::Error(e),

            Impulse::Start(_, _, _) => {
                panic!("no automatic conversion for start")
            },

            Impulse::Probe(tx) => Impulse::Probe(tx),
        }
    }
}

/// a singular cell of functionality that can be ported between organelles
///
/// you can think of a soma as a stream of impulses folded over a structure.
/// somas will perform some type of update upon receiving an impulse, which can
/// then propagate to other somas. when stitched together inside an organelle,
/// this can essentially be used to easily solve any asynchronous programming
/// problem in an efficient, modular, and scalable way.
pub trait Soma: Sized {
    /// the synapse a synapse plays in a connection between somas.
    type Synapse: Synapse;
    /// the types of errors that this soma can return
    type Error: std::error::Error + Send + Into<Error>;

    /// probe the internal structure of this soma
    #[async(boxed)]
    fn probe_data(self) -> std::result::Result<(Self, SomaData), Self::Error>
    where
        Self: 'static,
    {
        Ok((
            self,
            SomaData::Soma {
                synapse: Self::Synapse::data(),
                name: unsafe { intrinsics::type_name::<Self>().to_string() },
            },
        ))
    }

    /// react to a single impulse
    fn update(
        self,
        imp: Impulse<Self::Synapse>,
    ) -> Box<Future<Item = Self, Error = Self::Error>>;

    /// convert this soma into a future that can be passed to an event loop
    #[async(boxed)]
    fn run(mut self, handle: reactor::Handle) -> Result<()>
    where
        Self: 'static,
    {
        // it's important that tx live through this function
        let (tx, rx) = mpsc::channel(1);

        let uuid = Uuid::new_v4();

        await!(
            tx.clone()
                .send(Impulse::Start(uuid, tx, handle))
                .map_err(|_| Error::from("unable to send start signal"))
        )?;

        #[async]
        for imp in rx.map_err(|_| -> Error { unreachable!() }) {
            match imp {
                Impulse::Error(e) => bail!(e),
                Impulse::Stop => break,

                _ => self = await!(self.update(imp)).map_err(|e| e.into())?,
            }
        }

        Ok(())
    }
}
