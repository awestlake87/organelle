
#[macro_use]
extern crate error_chain;

extern crate cortical;

use cortical::*;

#[derive(Debug)]
enum IncrementerMessage {
    Increment,
    Ack,
}

impl From<CounterMessage> for IncrementerMessage {
    fn from(msg: CounterMessage) -> IncrementerMessage {
        match msg {
            CounterMessage::Ack => IncrementerMessage::Ack,
            msg @ _ => panic!(
                "counter does not support {:#?}", msg
            ),
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum IncrementerRole {
    Incrementer,
    Counter,
}

impl From<CounterRole> for IncrementerRole {
    fn from(c: CounterRole) -> IncrementerRole {
        match c {
            CounterRole::Incrementer => IncrementerRole::Incrementer,
            CounterRole::Counter => IncrementerRole::Counter,
        }
    }
}

type IncrementerSoma = Soma<IncrementerMessage, IncrementerRole>;

struct IncrementerLobe {
    soma:       IncrementerSoma,
}

impl IncrementerLobe {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: IncrementerSoma::new(
                    vec![ ],
                    vec![
                        Constraint::RequireOne(IncrementerRole::Incrementer)
                    ]
                )?,
            }
        )
    }
}

impl Lobe for IncrementerLobe {
    type Message = IncrementerMessage;
    type Role = IncrementerRole;

    fn update(mut self, msg: Protocol<Self::Message, Self::Role>)
        -> Result<Self>
    {
        self.soma.update(&msg)?;

        match msg {
            Protocol::Init(effector) => {
                println!("incrementer: {}", effector.this_lobe());
            },
            Protocol::AddOutput(output, role) => {
                println!(
                    "incrementer output {} {:#?}", output, role
                );
            },

            Protocol::Start => {
                self.soma.send_req_output(
                    IncrementerRole::Incrementer, IncrementerMessage::Increment
                )?;
            },

            Protocol::Message(_, IncrementerMessage::Ack) => {
                self.soma.send_req_output(
                    IncrementerRole::Incrementer, IncrementerMessage::Increment
                )?;
            },

            _ => (),
        }

        Ok(self)
    }
}

#[derive(Debug)]
enum CounterMessage {
    BumpCounter,
    Ack,
}

impl From<IncrementerMessage> for CounterMessage {
    fn from(msg: IncrementerMessage) -> CounterMessage {
        match msg {
            IncrementerMessage::Increment => CounterMessage::BumpCounter,
            msg @ _ => panic!(
                "counter does not support {:#?}", msg
            ),
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum CounterRole {
    Incrementer,
    Counter,
}

impl From<IncrementerRole> for CounterRole {
    fn from(role: IncrementerRole) -> CounterRole {
        match role {
            IncrementerRole::Incrementer => CounterRole::Incrementer,
            IncrementerRole::Counter => CounterRole::Counter,
        }
    }
}

type CounterSoma = Soma<CounterMessage, CounterRole>;

struct CounterLobe {
    soma: CounterSoma,

    counter: u32
}

impl CounterLobe {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: CounterSoma::new(
                    vec![ Constraint::RequireOne(CounterRole::Incrementer) ],
                    vec![ ]
                )?,

                counter: 0
            }
        )
    }
}

impl Lobe for CounterLobe {
    type Message = CounterMessage;
    type Role = CounterRole;

    fn update(mut self, msg: Protocol<Self::Message, Self::Role>)
        -> Result<Self>
    {
        self.soma.update(&msg)?;

        match msg {
            Protocol::Init(effector) => {
                println!("counter: {}", effector.this_lobe());
            },
            Protocol::AddInput(input, role) => {
                println!("counter input {} {:#?}", input, role);
            },

            Protocol::Message(_, CounterMessage::BumpCounter) => {
                if self.counter < 5 {
                    println!("counter increment");

                    self.counter += 1;
                    self.soma.send_req_input(
                        CounterRole::Incrementer, CounterMessage::Ack
                    )?;
                }
                else {
                    println!("stop");
                    self.soma.effector()?.stop();
                }
            },

            _ => (),
        }

        Ok(self)
    }
}

struct ForwarderLobe {
    soma: CounterSoma,
}

impl ForwarderLobe {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: CounterSoma::new(
                    vec![ Constraint::RequireOne(CounterRole::Incrementer) ],
                    vec![ Constraint::RequireOne(CounterRole::Incrementer) ],
                )?,
            }
        )
    }
}

impl Lobe for ForwarderLobe {
    type Message = CounterMessage;
    type Role = CounterRole;

    fn update(mut self, msg: Protocol<Self::Message, Self::Role>)
        -> Result<Self>
    {
        self.soma.update(&msg)?;

        match msg {
            Protocol::Init(effector) => {
                println!("forwarder: {}", effector.this_lobe());
            },
            Protocol::AddInput(input, _) => {
                println!("forwarder input: {}", input);
            },
            Protocol::AddOutput(output, _) => {
                println!("forwarder output: {}", output);
            },

            Protocol::Message(src, msg) => {
                if src == self.soma.req_input(CounterRole::Incrementer)? {
                    println!(
                        "forwarding input {:#?} through {}",
                        msg,
                        self.soma.effector()?.this_lobe()
                    );

                    self.soma.send_req_output(CounterRole::Incrementer, msg)?;
                }
                else if
                    src == self.soma.req_output(CounterRole::Incrementer)?
                {
                    println!(
                        "forwarding output {:#?} through {}",
                        msg,
                        self.soma.effector()?.this_lobe()
                    );

                    self.soma.send_req_input(CounterRole::Incrementer, msg)?;
                }
            },

            _ => ()
        }

        Ok(self)
    }
}

#[test]
fn test_cortex() {
    let mut cortex = Cortex::new(IncrementerLobe::new().unwrap());

    let counter = cortex.add_lobe(CounterLobe::new().unwrap());

    let main = cortex.get_main_handle();
    println!("cortex {}", main);
    cortex.connect(main, counter, IncrementerRole::Incrementer);

    run(cortex).unwrap();
}

#[test]
fn test_sub_cortex() {
    let mut counter_cortex = Cortex::new(ForwarderLobe::new().unwrap());

    let forwarder = counter_cortex.get_main_handle();
    let counter = counter_cortex.add_lobe(CounterLobe::new().unwrap());

    counter_cortex.connect(forwarder, counter, CounterRole::Incrementer);

    let mut inc_cortex = Cortex::new(IncrementerLobe::new().unwrap());

    let incrementer = inc_cortex.get_main_handle();
    let counter = inc_cortex.add_lobe(counter_cortex);
    // connect the incrementer to the counter cortex
    inc_cortex.connect(
        incrementer, counter, IncrementerRole::Incrementer
    );

    run(inc_cortex).unwrap();
}

struct InitErrorLobe {

}

impl Lobe for InitErrorLobe {
    type Message = IncrementerMessage;
    type Role = IncrementerRole;

    fn update(self, msg: Protocol<Self::Message, Self::Role>) -> Result<Self> {
        match msg {
            Protocol::Init(effector) => {
                effector.error("a lobe error!".into());

                Ok(self)
            },

            _ => Ok(self),
        }
    }
}

struct UpdateErrorLobe {

}

impl Lobe for UpdateErrorLobe {
    type Message = IncrementerMessage;
    type Role = IncrementerRole;

    fn update(self, _: Protocol<Self::Message, Self::Role>) -> Result<Self> {
        bail!("update failed")
    }
}

#[test]
fn test_lobe_error() {
    if let Ok(_) = run(InitErrorLobe { }) {
        panic!("lobe init was supposed to fail");
    }

    if let Ok(_) = run(UpdateErrorLobe { }) {
        panic!("lobe update was supposed to fail");
    }

    if let Ok(_) = run(Cortex::new(UpdateErrorLobe { })) {
        panic!("cortex updates were supposed to fail");
    }
}
