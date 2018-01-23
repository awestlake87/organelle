use std;
use std::collections::HashMap;
use std::mem;

use futures::future;
use futures::prelude::*;
use futures::unsync;
use tokio_core::reactor;
use uuid::Uuid;

use super::{Error, Impulse, Result, Role, Soma};

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
    main_tx: unsync::mpsc::Sender<Impulse<T::Role>>,
    main_rx: Option<unsync::mpsc::Receiver<Impulse<T::Role>>>,

    somas: HashMap<Uuid, unsync::mpsc::Sender<Impulse<T::Role>>>,
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
    pub fn nucleus(&self) -> Uuid {
        self.main
    }

    fn create_soma_channel<R>(
        &mut self,
    ) -> (Uuid, unsync::mpsc::Receiver<Impulse<R>>)
    where
        R: Role + From<T::Role> + Into<T::Role> + 'static,
        R::Dendrite: From<<T::Role as Role>::Dendrite>
            + Into<<T::Role as Role>::Dendrite>
            + 'static,
        R::Terminal: From<<T::Role as Role>::Terminal>
            + Into<<T::Role as Role>::Terminal>
            + 'static,
    {
        let uuid = Uuid::new_v4();

        let (tx, rx) = unsync::mpsc::channel::<Impulse<T::Role>>(10);

        let (soma_tx, soma_rx) = unsync::mpsc::channel::<Impulse<R>>(1);

        self.handle.spawn(rx.for_each(move |imp| {
            soma_tx
                .clone()
                .send(match imp {
                    Impulse::Start(sender, handle) => {
                        let (tx, rx) = unsync::mpsc::channel::<Impulse<R>>(1);

                        handle.spawn(rx.for_each(move |imp| {
                            sender
                                .clone()
                                .send(Impulse::<T::Role>::convert_from(imp))
                                .then(|_| future::ok(()))
                        }).then(|_| future::ok(())));

                        Impulse::Start(tx, handle)
                    },
                    _ => Impulse::<R>::convert_from(imp),
                })
                .map(|_| ())
                .map_err(|_| ())
        }).map_err(|_| ()));

        self.somas.insert(uuid, tx);

        (uuid, soma_rx)
    }

    #[async]
    fn run_soma<U: Soma + 'static>(
        mut soma: U,
        soma_rx: unsync::mpsc::Receiver<Impulse<U::Role>>,
    ) -> std::result::Result<(), Error> {
        #[async]
        for imp in soma_rx.map_err(|_| Error::from("streams can't fail")) {
            soma = await!(soma.update(imp)).map_err(|e| e.into())?;
        }

        Ok(())
    }

    /// add a soma to the organelle
    pub fn add_soma<U: Soma + 'static>(&mut self, soma: U) -> Uuid
    where
        U::Role: From<T::Role> + Into<T::Role>,
        <U::Role as Role>::Dendrite: From<<T::Role as Role>::Dendrite>
            + Into<<T::Role as Role>::Dendrite>,
        <U::Role as Role>::Terminal: From<<T::Role as Role>::Terminal>
            + Into<<T::Role as Role>::Terminal>,
    {
        let (uuid, soma_rx) = self.create_soma_channel::<U::Role>();

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

    /// connect two somas together using the specified role
    pub fn connect(
        &self,
        input: Uuid,
        output: Uuid,
        role: T::Role,
    ) -> Result<()> {
        let (tx, rx) = role.synapse();

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
                .send(Impulse::AddTerminal(role, tx))
                .then(|_| future::ok(())),
        );
        self.handle.spawn(
            output_sender
                .send(Impulse::AddDendrite(role, rx))
                .then(|_| future::ok(())),
        );

        Ok(())
    }

    fn start_all(
        &self,
        tx: unsync::mpsc::Sender<Impulse<T::Role>>,
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
    type Error = Error;
    type Future = Box<Future<Item = Self, Error = Self::Error>>;

    #[async(boxed)]
    fn update(self, imp: Impulse<T::Role>) -> Result<Self> {
        match imp {
            Impulse::AddDendrite(_, _) | Impulse::AddTerminal(_, _) => {
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
