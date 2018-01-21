#![warn(missing_docs)]
#![feature(core_intrinsics, proc_macro, conservative_impl_trait, generators)]

//! Organelle - reactive architecture for emergent AI systems

#[macro_use]
extern crate error_chain;

extern crate futures_await as futures;
extern crate tokio;
extern crate tokio_core;
extern crate uuid;

use std::collections::HashMap;
use std::fmt::Debug;

use futures::future;
use futures::prelude::*;
use futures::unsync;
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

/// trait alias to express requirements of a Role type
pub trait Role: Debug + Copy + Clone {}

impl<T> Role for T
where
    T: Debug + Copy + Clone,
{
}

/// trait alias to express requirements of a Synapse type
pub trait Synapse {}

impl<T> Synapse for T {}

/// a group of control signals passed between somas
pub enum Impulse<R: Role, S: Synapse> {
    /// add an input synapse with the given role to the soma
    ///
    /// you should always expect to handle this impulse if the soma has any
    /// inputs. if your soma has inputs, it is best to wrap it with an Axon
    /// which can be used for validation purposes.
    AddInput(R, S),
    /// add an output synapse with the given role to the soma
    ///
    /// you should always expect to handle this impulse if the soma has any
    /// outputs. if your soma has outputs, it is best to wrap it with an Axon
    /// which can be used for validation purposes.
    AddOutput(R, S),
    /// notify the soma that it has received all of its inputs and outputs
    ///
    /// you should always expect to handle this impulse because it will be
    /// passed to each soma regardless of configuration
    Start(unsync::mpsc::Sender<Impulse<R, S>>, reactor::Handle),
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
}

impl<R, S> Impulse<R, S>
where
    R: Role,
    S: Synapse,
{
    fn convert_from<T, U>(imp: Impulse<T, U>) -> Self
    where
        T: Role + Into<R>,
        U: Synapse + Into<S>,
    {
        match imp {
            Impulse::AddInput(role, synapse) => {
                Impulse::AddInput(role.into(), synapse.into())
            },
            Impulse::AddOutput(role, synapse) => {
                Impulse::AddOutput(role.into(), synapse.into())
            },
            Impulse::Stop => Impulse::Stop,
            Impulse::Error(e) => Impulse::Error(e),

            Impulse::Start(_, _) => panic!("no automatic conversion for start"),
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
    /// the role a synapse plays in a connection between somas.
    type Role: Role + Into<(Self::Synapse, Self::Synapse)>;
    /// the glue that binds somas together.
    ///
    /// this will (probably) be an enum representing the different types of
    /// connections that can be made between this soma and others. synapses can
    /// be used to exchange synchronization primitives such as (but not limited
    /// to) mpsc and oneshot channels. these can provide custom-tailored methods
    /// of communication between somas to ease the pain of async programming.
    type Synapse: Synapse;
    /// the types of errors that this soma can return
    type Error: std::error::Error + Send;
    /// the future representing a single update of the soma.
    type Future: Future<Item = Self, Error = Self::Error>;

    /// react to a single impulse
    fn update(self, imp: Impulse<Self::Role, Self::Synapse>) -> Self::Future;

    /// convert this soma into a future that can be passed to an event loop
    #[async(boxed)]
    fn run(mut self, handle: reactor::Handle) -> Result<()>
    where
        Self: 'static,
    {
        // it's important that tx live through this function
        let (tx, rx) = unsync::mpsc::channel(1);

        await!(
            tx.clone()
                .send(Impulse::Start(tx, handle))
                .map_err(|_| Error::from("unable to send start signal"))
        )?;

        #[async]
        for imp in rx.map_err(|_| Error::from("streams can't fail")) {
            match imp {
                Impulse::Error(e) => bail!(e),
                Impulse::Stop => break,

                _ => {
                    self = await!(self.update(imp))
                        .chain_err(|| ErrorKind::SomaError)?
                },
            }
        }

        Ok(())
    }
}

/// a soma designed to facilitate connections between other somas
///
/// where somas are the single cells of functionality, organelles are the
/// organisms capable of more complex tasks. however, organelles are still
/// essentially somas, so they can used in larger organelles as long as they
/// comply with their standards.
pub struct Organelle<T: Soma>
where
    T: Soma,
{
    handle: reactor::Handle,

    main: Uuid,

    somas: HashMap<Uuid, unsync::mpsc::Sender<Impulse<T::Role, T::Synapse>>>,
}

impl<T: Soma + 'static> Organelle<T> {
    /// create a new organelle
    pub fn new(main: T, handle: reactor::Handle) -> Self {
        let mut organelle = Self {
            handle: handle,

            main: Uuid::new_v4(),

            somas: HashMap::new(),
        };

        let main = organelle.add_soma(main);
        organelle.main = main;

        organelle
    }

    /// get the main soma's uuid
    pub fn main(&self) -> Uuid {
        self.main
    }

    /// add a soma to the organelle
    pub fn add_soma<U: Soma + 'static>(&mut self, mut soma: U) -> Uuid
    where
        U::Role: From<T::Role> + Into<T::Role>,
        U::Synapse: From<T::Synapse> + Into<T::Synapse>,
    {
        let uuid = Uuid::new_v4();

        let (tx, rx) =
            unsync::mpsc::channel::<Impulse<T::Role, T::Synapse>>(10);

        self.handle.spawn(
            async_block! {
                #[async]
                for imp in rx.map_err(|_| Error::from("streams can't fail")) {
                    soma = await!(soma.update(match imp {
                        Impulse::Start(sender, handle) =>{
                            let (tx, rx) = unsync::mpsc::channel(1);
                            handle.spawn(
                                rx.for_each(move |imp| {
                                    sender.clone().send(
                                        Impulse::<
                                            T::Role,
                                            T::Synapse,
                                        >::convert_from(imp)
                                    ).then(|_| future::ok(()))
                                })
                                .then(|_| future::ok(())),
                            );

                            Impulse::Start(tx, handle)
                        },
                        _ => Impulse::<U::Role, U::Synapse>::convert_from(imp)
                    })).chain_err(|| ErrorKind::SomaError)?;
                }

                Ok(())
            }.map_err(|e: Error| panic!("{:?}", e)),
        );

        self.somas.insert(uuid, tx);

        uuid
    }

    /// connect two somas together using the specified role
    pub fn connect(
        &self,
        input: Uuid,
        output: Uuid,
        role: T::Role,
    ) -> Result<()> {
        let (tx, rx) = role.into();

        let input_sender = if let Some(sender) = self.somas.get(&input) {
            sender.clone()
        } else {
            bail!("unable to find input")
        };

        let output_sender = if let Some(sender) = self.somas.get(&output) {
            sender.clone()
        } else {
            bail!("unable to find output")
        };

        self.handle.spawn(
            input_sender
                .send(Impulse::AddOutput(role, tx))
                .then(|_| future::ok(())),
        );
        self.handle.spawn(
            output_sender
                .send(Impulse::AddInput(role, rx))
                .then(|_| future::ok(())),
        );

        Ok(())
    }

    fn start_all(
        &self,
        tx: unsync::mpsc::Sender<Impulse<T::Role, T::Synapse>>,
        handle: reactor::Handle,
    ) -> Result<()> {
        for sender in self.somas.values() {
            self.handle.spawn(
                sender
                    .clone()
                    .send(Impulse::Start(tx.clone(), handle.clone()))
                    .then(|_| future::ok(())),
            );
        }

        Ok(())
    }
}

impl<T: Soma + 'static> Soma for Organelle<T> {
    type Role = T::Role;
    type Synapse = T::Synapse;
    type Error = Error;
    type Future = Box<Future<Item = Self, Error = Self::Error>>;

    #[async(boxed)]
    fn update(self, imp: Impulse<T::Role, T::Synapse>) -> Result<Self> {
        match imp {
            Impulse::Start(tx, handle) => {
                self.start_all(tx, handle)?;

                Ok(self)
            },

            _ => unimplemented!(),
        }
    }
}

/// constraints that can be put on axons for validation purposes
pub enum Dendrite<R> {
    /// only accept one synapse for the given role
    One(R),
}

/// wrap a soma with a set of requirements that will be validated upon startup
pub struct Axon<T: Soma + 'static> {
    soma: T,
    inputs: Vec<Dendrite<T::Role>>,
    outputs: Vec<Dendrite<T::Role>>,
}

impl<T: Soma + 'static> Axon<T> {
    /// wrap a soma with constraints specified by input and output dendrites
    pub fn new(
        soma: T,
        inputs: Vec<Dendrite<T::Role>>,
        outputs: Vec<Dendrite<T::Role>>,
    ) -> Self {
        Self {
            soma: soma,
            inputs: inputs,
            outputs: outputs,
        }
    }

    fn add_input(&self, role: T::Role) -> Result<()> {
        println!("add input role {:?}", role);
        Ok(())
    }

    fn add_output(&self, role: T::Role) -> Result<()> {
        println!("add output role {:?}", role);
        Ok(())
    }

    fn start(&self) -> Result<()> {
        println!("axon validate");
        Ok(())
    }
}

impl<T: Soma + 'static> Soma for Axon<T> {
    type Role = T::Role;
    type Synapse = T::Synapse;
    type Error = Error;
    type Future = Box<Future<Item = Self, Error = Self::Error>>;

    #[async(boxed)]
    fn update(self, imp: Impulse<T::Role, T::Synapse>) -> Result<Self> {
        Ok(Self {
            soma: match imp {
                Impulse::AddInput(role, _) => {
                    self.add_input(role)?;

                    await!(self.soma.update(imp))
                        .chain_err(|| ErrorKind::SomaError)?
                },
                Impulse::AddOutput(role, _) => {
                    self.add_output(role)?;

                    await!(self.soma.update(imp))
                        .chain_err(|| ErrorKind::SomaError)?
                },
                Impulse::Start(_, _) => {
                    self.start()?;

                    await!(self.soma.update(imp))
                        .chain_err(|| ErrorKind::SomaError)?
                },

                Impulse::Stop | Impulse::Error(_) => {
                    bail!("unexpected impulse in axon")
                },
                //_ => await!(self.soma.update(imp))?,
            },
            inputs: self.inputs,
            outputs: self.outputs,
        })
    }
}
