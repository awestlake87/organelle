
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

struct IncrementerSoma;

impl IncrementerSoma {
    fn sheath() -> Result<Sheath<Self>> {
        Sheath::new(
            Self { },
            vec![ ],
            vec![
                Dendrite::RequireOne(IncrementerSynapse::Incrementer)
            ],
        )
    }
}

impl Neuron for IncrementerSoma {
    type Signal = IncrementerSignal;
    type Synapse = IncrementerSynapse;

    fn update(
        self,
        axon: &Axon<Self::Signal, Self::Synapse>,
        msg: Impulse<Self::Signal, Self::Synapse>,
    )
        -> Result<Self>
    {
        match msg {
            Impulse::Start => {
                axon.send_req_output(
                    IncrementerSynapse::Incrementer,
                    IncrementerSignal::Increment
                )?;

                Ok(self)
            },

            Impulse::Signal(_, IncrementerSignal::Ack) => {
                axon.send_req_output(
                    IncrementerSynapse::Incrementer,
                    IncrementerSignal::Increment
                )?;

                Ok(self)
            },

            _ => bail!("unexpected message"),
        }
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

struct CounterSoma {
    counter: u32
}

impl CounterSoma {
    fn sheath() -> Result<Sheath<Self>> {
        Sheath::new(
            Self { counter: 0 },
            vec![ Dendrite::RequireOne(CounterSynapse::Incrementer) ],
            vec![ ],
        )
    }
}

impl Neuron for CounterSoma {
    type Signal = CounterSignal;
    type Synapse = CounterSynapse;

    fn update(
        mut self,
        axon: &Axon<Self::Signal, Self::Synapse>,
        msg: Impulse<Self::Signal, Self::Synapse>,
    )
        -> Result<Self>
    {
        match msg {
            Impulse::Start => Ok(self),

            Impulse::Signal(_, CounterSignal::BumpCounter) => {
                if self.counter < 5 {
                    println!("counter increment");

                    self.counter += 1;
                    axon.send_req_input(
                        CounterSynapse::Incrementer, CounterSignal::Ack
                    )?;
                }
                else {
                    println!("stop");
                    axon.effector()?.stop();
                }

                Ok(self)
            },

            _ => bail!("unexpected message"),
        }
    }
}

struct ForwarderSoma;

impl ForwarderSoma {
    fn sheath() -> Result<Sheath<Self>> {
        Sheath::new(
            Self { },
            vec![ Dendrite::RequireOne(CounterSynapse::Incrementer) ],
            vec![ Dendrite::RequireOne(CounterSynapse::Incrementer) ],
        )
    }
}

impl Neuron for ForwarderSoma {
    type Signal = CounterSignal;
    type Synapse = CounterSynapse;

    fn update(
        self,
        axon: &Axon<Self::Signal, Self::Synapse>,
        msg: Impulse<Self::Signal, Self::Synapse>,
    )
        -> Result<Self>
    {
        match msg {
            Impulse::Start => Ok(self),

            Impulse::Signal(src, msg) => {
                if src == axon.req_input(CounterSynapse::Incrementer)? {
                    println!(
                        "forwarding input {:#?} through {}",
                        msg,
                        axon.effector()?.this_soma()
                    );

                    axon.send_req_output(
                        CounterSynapse::Incrementer, msg
                    )?;
                }
                else if
                    src == axon.req_output(CounterSynapse::Incrementer)?
                {
                    println!(
                        "forwarding output {:#?} through {}",
                        msg,
                        axon.effector()?.this_soma()
                    );

                    axon.send_req_input(
                        CounterSynapse::Incrementer, msg
                    )?;
                }

                Ok(self)
            },

            _ => bail!("unexpected message")
        }
    }
}

#[test]
fn test_organelle() {
    let mut organelle = Organelle::new(IncrementerSoma::sheath().unwrap());

    let counter = organelle.add_soma(CounterSoma::sheath().unwrap());

    let main = organelle.get_main_handle();
    println!("organelle {}", main);
    organelle.connect(main, counter, IncrementerSynapse::Incrementer);

    organelle.run().unwrap();
}

#[test]
fn test_sub_organelle() {
    let mut counter_organelle = Organelle::new(
        ForwarderSoma::sheath().unwrap()
    );

    let forwarder = counter_organelle.get_main_handle();
    let counter = counter_organelle.add_soma(CounterSoma::sheath().unwrap());

    counter_organelle.connect(forwarder, counter, CounterSynapse::Incrementer);

    let mut inc_organelle = Organelle::new(IncrementerSoma::sheath().unwrap());

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
