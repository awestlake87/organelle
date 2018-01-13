
#[macro_use]
extern crate error_chain;

extern crate organelle;

use organelle::*;

#[derive(Debug)]
enum IncrementerSignal {
    Increment,
    Ack,
}

impl From<CounterSignal> for IncrementerSignal {
    fn from(msg: CounterSignal) -> IncrementerSignal {
        match msg {
            CounterSignal::Ack => IncrementerSignal::Ack,
            msg @ _ => panic!(
                "counter does not support {:#?}", msg
            ),
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum IncrementerSynapse {
    Incrementer,
    Counter,
}

impl From<CounterSynapse> for IncrementerSynapse {
    fn from(c: CounterSynapse) -> IncrementerSynapse {
        match c {
            CounterSynapse::Incrementer => IncrementerSynapse::Incrementer,
            CounterSynapse::Counter => IncrementerSynapse::Counter,
        }
    }
}

type IncrementerAxon = Axon<IncrementerSignal, IncrementerSynapse>;

struct IncrementerSoma {
    soma:       IncrementerAxon,
}

impl IncrementerSoma {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: IncrementerAxon::new(
                    vec![ ],
                    vec![
                        Dendrite::RequireOne(IncrementerSynapse::Incrementer)
                    ]
                )?,
            }
        )
    }
}

impl Soma for IncrementerSoma {
    type Signal = IncrementerSignal;
    type Synapse = IncrementerSynapse;

    fn update(mut self, msg: Impulse<Self::Signal, Self::Synapse>)
        -> Result<Self>
    {
        if let Some(msg) = self.soma.update(msg)? {
            match msg {
                Impulse::Start => {
                    self.soma.send_req_output(
                        IncrementerSynapse::Incrementer,
                        IncrementerSignal::Increment
                    )?;
                },

                Impulse::Signal(_, IncrementerSignal::Ack) => {
                    self.soma.send_req_output(
                        IncrementerSynapse::Incrementer,
                        IncrementerSignal::Increment
                    )?;
                },

                _ => bail!("unexpected message"),
            }
        }

        Ok(self)
    }
}

#[derive(Debug)]
enum CounterSignal {
    BumpCounter,
    Ack,
}

impl From<IncrementerSignal> for CounterSignal {
    fn from(msg: IncrementerSignal) -> CounterSignal {
        match msg {
            IncrementerSignal::Increment => CounterSignal::BumpCounter,
            msg @ _ => panic!(
                "counter does not support {:#?}", msg
            ),
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum CounterSynapse {
    Incrementer,
    Counter,
}

impl From<IncrementerSynapse> for CounterSynapse {
    fn from(role: IncrementerSynapse) -> CounterSynapse {
        match role {
            IncrementerSynapse::Incrementer => CounterSynapse::Incrementer,
            IncrementerSynapse::Counter => CounterSynapse::Counter,
        }
    }
}

type CounterAxon = Axon<CounterSignal, CounterSynapse>;

struct CounterSoma {
    soma: CounterAxon,

    counter: u32
}

impl CounterSoma {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: CounterAxon::new(
                    vec![ Dendrite::RequireOne(CounterSynapse::Incrementer) ],
                    vec![ ]
                )?,

                counter: 0
            }
        )
    }
}

impl Soma for CounterSoma {
    type Signal = CounterSignal;
    type Synapse = CounterSynapse;

    fn update(mut self, msg: Impulse<Self::Signal, Self::Synapse>)
        -> Result<Self>
    {
        if let Some(msg) = self.soma.update(msg)? {
            match msg {
                Impulse::Start => (),

                Impulse::Signal(_, CounterSignal::BumpCounter) => {
                    if self.counter < 5 {
                        println!("counter increment");

                        self.counter += 1;
                        self.soma.send_req_input(
                            CounterSynapse::Incrementer, CounterSignal::Ack
                        )?;
                    }
                    else {
                        println!("stop");
                        self.soma.effector()?.stop();
                    }
                },

                _ => bail!("unexpected message"),
            }
        }

        Ok(self)
    }
}

struct ForwarderSoma {
    soma: CounterAxon,
}

impl ForwarderSoma {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: CounterAxon::new(
                    vec![ Dendrite::RequireOne(CounterSynapse::Incrementer) ],
                    vec![ Dendrite::RequireOne(CounterSynapse::Incrementer) ],
                )?,
            }
        )
    }
}

impl Soma for ForwarderSoma {
    type Signal = CounterSignal;
    type Synapse = CounterSynapse;

    fn update(mut self, msg: Impulse<Self::Signal, Self::Synapse>)
        -> Result<Self>
    {
        if let Some(msg) = self.soma.update(msg)? {
            match msg {
                Impulse::Start => (),

                Impulse::Signal(src, msg) => {
                    if src == self.soma.req_input(CounterSynapse::Incrementer)? {
                        println!(
                            "forwarding input {:#?} through {}",
                            msg,
                            self.soma.effector()?.this_soma()
                        );

                        self.soma.send_req_output(
                            CounterSynapse::Incrementer, msg
                        )?;
                    }
                    else if
                        src == self.soma.req_output(CounterSynapse::Incrementer)?
                    {
                        println!(
                            "forwarding output {:#?} through {}",
                            msg,
                            self.soma.effector()?.this_soma()
                        );

                        self.soma.send_req_input(
                            CounterSynapse::Incrementer, msg
                        )?;
                    }
                },

                _ => bail!("unexpected message")
            }
        }

        Ok(self)
    }
}

#[test]
fn test_organelle() {
    let mut organelle = Organelle::new(IncrementerSoma::new().unwrap());

    let counter = organelle.add_soma(CounterSoma::new().unwrap());

    let main = organelle.get_main_handle();
    println!("organelle {}", main);
    organelle.connect(main, counter, IncrementerSynapse::Incrementer);

    organelle.run().unwrap();
}

#[test]
fn test_sub_organelle() {
    let mut counter_organelle = Organelle::new(ForwarderSoma::new().unwrap());

    let forwarder = counter_organelle.get_main_handle();
    let counter = counter_organelle.add_soma(CounterSoma::new().unwrap());

    counter_organelle.connect(forwarder, counter, CounterSynapse::Incrementer);

    let mut inc_organelle = Organelle::new(IncrementerSoma::new().unwrap());

    let incrementer = inc_organelle.get_main_handle();
    let counter = inc_organelle.add_soma(counter_organelle);
    // connect the incrementer to the counter organelle
    inc_organelle.connect(
        incrementer, counter, IncrementerSynapse::Incrementer
    );

    inc_organelle.run().unwrap();
}

struct InitErrorSoma {

}

impl InitErrorSoma {
    fn new() -> Self {
        Self { }
    }
}

impl Soma for InitErrorSoma {
    type Signal = IncrementerSignal;
    type Synapse = IncrementerSynapse;

    fn update(self, msg: Impulse<Self::Signal, Self::Synapse>) -> Result<Self> {
        match msg {
            Impulse::Init(effector) => {
                effector.error("a soma error!".into());

                Ok(self)
            },

            _ => Ok(self),
        }
    }
}

struct UpdateErrorSoma {

}

impl UpdateErrorSoma {
    fn new() -> Self {
        Self { }
    }
}

impl Soma for UpdateErrorSoma {
    type Signal = IncrementerSignal;
    type Synapse = IncrementerSynapse;

    fn update(self, _: Impulse<Self::Signal, Self::Synapse>) -> Result<Self> {
        bail!("update failed")
    }
}

#[test]
fn test_soma_error() {
    if let Ok(_) = InitErrorSoma::new().run() {
        panic!("soma init was supposed to fail");
    }

    if let Ok(_) = UpdateErrorSoma::new().run() {
        panic!("soma update was supposed to fail");
    }

    if let Ok(_) = Organelle::new(UpdateErrorSoma { }).run() {
        panic!("organelle updates were supposed to fail");
    }
}
