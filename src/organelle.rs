use std;
use std::collections::HashMap;
use std::intrinsics;
use std::mem;

use futures::future;
use futures::prelude::*;
use futures::stream;
use futures::unsync::{mpsc, oneshot};
use tokio_core::reactor;
use uuid::Uuid;

use super::{Error, Result};
use probe::{self, SomaData};
use soma::{Impulse, Soma, Synapse};

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

    uuid: Option<Uuid>,

    main: Uuid,
    main_tx: mpsc::Sender<Impulse<T::Synapse>>,
    main_rx: Option<mpsc::Receiver<Impulse<T::Synapse>>>,

    somas: HashMap<Uuid, mpsc::Sender<Impulse<T::Synapse>>>,
}

impl<T: Soma + 'static> Organelle<T> {
    /// create a new organelle
    pub fn new(main: T, handle: reactor::Handle) -> Self {
        let (tx, rx) = mpsc::channel(100);

        let mut organelle = Self {
            handle: handle,

            uuid: None,

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
    pub fn nucleus(&self) -> Uuid {
        self.main
    }

    fn create_soma_channel<R>(&mut self) -> (Uuid, mpsc::Receiver<Impulse<R>>)
    where
        R: Synapse + From<T::Synapse> + Into<T::Synapse> + 'static,
        R::Dendrite: From<<T::Synapse as Synapse>::Dendrite>
            + Into<<T::Synapse as Synapse>::Dendrite>
            + 'static,
        R::Terminal: From<<T::Synapse as Synapse>::Terminal>
            + Into<<T::Synapse as Synapse>::Terminal>
            + 'static,
    {
        let uuid = Uuid::new_v4();

        let (tx, rx) = mpsc::channel::<Impulse<T::Synapse>>(10);

        let (soma_tx, soma_rx) = mpsc::channel::<Impulse<R>>(1);

        self.handle.spawn(
            soma_tx
                .send_all(rx.map(|imp| match imp {
                    Impulse::Start(uuid, sender, handle) => {
                        let (tx, rx) = mpsc::channel::<Impulse<R>>(1);

                        handle.spawn(
                            sender
                                .send_all(rx.map(move |imp| {
                                    Impulse::<T::Synapse>::convert_from(imp)
                                }).map_err(|_| unreachable!()))
                                .map(|_| ())
                                .map_err(|_| ()),
                        );

                        Impulse::Start(uuid, tx, handle)
                    },
                    _ => Impulse::<R>::convert_from(imp),
                }).map_err(|_| unreachable!()))
                .map(|_| ())
                .map_err(|_| ()),
        );

        self.somas.insert(uuid, tx);

        (uuid, soma_rx)
    }

    #[async]
    fn run_soma<U: Soma + 'static>(
        mut soma: U,
        soma_rx: mpsc::Receiver<Impulse<U::Synapse>>,
    ) -> std::result::Result<(), Error> {
        #[async]
        for imp in soma_rx.map_err(|_| -> Error { unreachable!() }) {
            soma = await!(soma.update(imp)).map_err(|e| e.into())?;
        }

        Ok(())
    }

    /// add a soma to the organelle
    pub fn add_soma<U: Soma + 'static>(&mut self, soma: U) -> Uuid
    where
        U::Synapse: From<T::Synapse> + Into<T::Synapse>,
        <U::Synapse as Synapse>::Dendrite: From<<T::Synapse as Synapse>::Dendrite>
            + Into<<T::Synapse as Synapse>::Dendrite>,
        <U::Synapse as Synapse>::Terminal: From<<T::Synapse as Synapse>::Terminal>
            + Into<<T::Synapse as Synapse>::Terminal>,
    {
        let (uuid, soma_rx) = self.create_soma_channel::<U::Synapse>();

        let main_tx = self.main_tx.clone();

        self.handle
            .spawn(Self::run_soma(soma, soma_rx).or_else(move |e| {
                main_tx
                    .send(Impulse::Error(e.into()))
                    .map(|_| ())
                    .map_err(|_| ())
            }));

        uuid
    }

    /// connect two somas together using the specified synapse
    pub fn connect(
        &self,
        dendrite: Uuid,
        terminal: Uuid,
        synapse: T::Synapse,
    ) -> Result<()> {
        let (tx, rx) = synapse.synapse();

        self.add_terminal((terminal, tx), dendrite, synapse)?;
        self.add_dendrite((dendrite, rx), terminal, synapse)?;

        Ok(())
    }

    /// send a dendrite to the specified soma
    pub fn add_dendrite(
        &self,
        dendrite: (Uuid, <T::Synapse as Synapse>::Dendrite),
        terminal: Uuid,
        synapse: T::Synapse,
    ) -> Result<()> {
        let terminal_sender = if let Some(sender) = self.somas.get(&terminal) {
            sender.clone()
        } else {
            bail!("unable to find terminal")
        };

        self.handle.spawn(
            terminal_sender
                .send(Impulse::AddDendrite(dendrite.0, synapse, dendrite.1))
                .map(|_| ())
                .map_err(|_| {
                    eprintln!("unable to add dendrite");
                }),
        );

        Ok(())
    }

    /// send a terminal to the specified soma
    pub fn add_terminal(
        &self,
        terminal: (Uuid, <T::Synapse as Synapse>::Terminal),
        dendrite: Uuid,
        synapse: T::Synapse,
    ) -> Result<()> {
        let dendrite_sender = if let Some(sender) = self.somas.get(&dendrite) {
            sender.clone()
        } else {
            bail!("unable to find dendrite")
        };

        self.handle.spawn(
            dendrite_sender
                .send(Impulse::AddTerminal(terminal.0, synapse, terminal.1))
                .map(|_| ())
                .map_err(|_| {
                    eprintln!("unable to add terminal");
                }),
        );

        Ok(())
    }

    fn start_all(
        &self,
        tx: mpsc::Sender<Impulse<T::Synapse>>,
        handle: reactor::Handle,
    ) -> Result<()> {
        for (uuid, sender) in &self.somas {
            self.handle.spawn(
                sender
                    .clone()
                    .send(Impulse::Start(*uuid, tx.clone(), handle.clone()))
                    .then(|_| future::ok(())),
            );
        }

        Ok(())
    }

    #[async]
    fn perform_probe(
        self,
        settings: probe::Settings,
        tx: oneshot::Sender<SomaData>,
    ) -> Result<Self> {
        let (organelle, data) = await!(self.probe(settings))?;

        if let Err(_) = tx.send(data) {
            // rx does not care anymore
        }

        Ok(organelle)
    }
}

impl<T: Soma + 'static> Soma for Organelle<T> {
    type Synapse = T::Synapse;
    type Error = Error;

    #[async(boxed)]
    fn probe(self, settings: probe::Settings) -> Result<(Self, SomaData)> {
        let results = await!(
            stream::iter_ok(self.somas.clone())
                .map(move |(uuid, sender)| {
                    let (tx, rx) = oneshot::channel();

                    sender
                        .send(Impulse::Probe(settings.clone(), tx))
                        .map_err(|_| {
                            Error::from("unable to send probe impulse")
                        })
                        .and_then(move |_| {
                            rx.map(move |rx| (uuid, rx)).map_err(|e| e.into())
                        })
                })
                .collect()
                .and_then(|receivers| future::join_all(receivers))
        )?;

        let nucleus_uuid = self.nucleus();
        let mut nucleus = None;

        let somas = results
            .into_iter()
            .filter_map(|(uuid, data)| {
                if uuid == nucleus_uuid {
                    nucleus = Some(data);
                    None
                } else {
                    Some(data)
                }
            })
            .collect();

        let uuid = self.uuid.unwrap();

        Ok((
            self,
            SomaData::Organelle {
                nucleus: Box::new(nucleus.unwrap()),
                somas: somas,
                uuid: uuid,
                name: unsafe { intrinsics::type_name::<Self>().into() },
            },
        ))
    }

    #[async(boxed)]
    fn update(mut self, imp: Impulse<T::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddDendrite(_, _, _) | Impulse::AddTerminal(_, _, _) => {
                await!(
                    self.somas
                        .get(&self.nucleus())
                        .unwrap()
                        .clone()
                        .send(imp)
                        .map_err(|_| Error::from("unable to forward impulse"))
                )?;
                Ok(self)
            },
            Impulse::Start(uuid, tx, handle) => {
                self.uuid = Some(uuid);

                let rx = mem::replace(&mut self.main_rx, None).unwrap();

                handle.spawn(
                    tx.clone()
                        .send_all(rx.map_err(|_| unreachable!()))
                        .map(|_| ())
                        .map_err(|_| ()),
                );

                self.start_all(tx, handle)?;

                Ok(self)
            },

            Impulse::Probe(settings, tx) => {
                await!(self.perform_probe(settings, tx))
            },

            Impulse::Stop | Impulse::Error(_) => unreachable!(),
        }
    }

    /// convert this soma into a future that can be passed to an event loop
    #[async(boxed)]
    fn run(mut self, handle: reactor::Handle) -> Result<()>
    where
        Self: 'static,
    {
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

                _ => {
                    self = await!(self.update(imp))
                        .map_err(|e| -> Error { e.into() })?
                },
            }
        }

        Ok(())
    }
}
