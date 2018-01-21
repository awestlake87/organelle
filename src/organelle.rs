use std::collections::HashMap;
use std::mem;

use futures::future;
use futures::prelude::*;
use futures::unsync;
use tokio_core::reactor;
use uuid::Uuid;

use super::{Error, Impulse, Result, Soma};

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
    main_tx: unsync::mpsc::Sender<Impulse<T::Role, T::Synapse>>,
    main_rx: Option<unsync::mpsc::Receiver<Impulse<T::Role, T::Synapse>>>,

    somas: HashMap<Uuid, unsync::mpsc::Sender<Impulse<T::Role, T::Synapse>>>,
}

impl<T: Soma + 'static> Organelle<T> {
    /// create a new organelle
    pub fn new(main: T, handle: reactor::Handle) -> Self {
        let (tx, rx) = unsync::mpsc::channel(100);

        let mut organelle = Self {
            handle: handle,

            main: Uuid::new_v4(),
            main_tx: tx,
            main_rx: Some(rx),

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

        let main_tx = self.main_tx.clone();

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
                    })).map_err(|e| e.into())?;
                }

                Ok(())
            }.or_else(|e: Error| {
                main_tx.send(Impulse::Error(e)).map(|_| ()).map_err(|_| ())
            }),
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

    /// convert this soma into a future that can be passed to an event loop
    #[async(boxed)]
    fn run(mut self, handle: reactor::Handle) -> Result<()>
    where
        Self: 'static,
    {
        // it's important that tx live through this function
        let (tx, rx) = (
            self.main_tx.clone(),
            mem::replace(&mut self.main_rx, None).unwrap(),
        );

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
                        .map_err(|e| -> Error { e.into() })?
                },
            }
        }

        Ok(())
    }
}
