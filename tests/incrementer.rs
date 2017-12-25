
#[macro_use]
extern crate error_chain;

extern crate cortical;

use cortical::*;

#[derive(Debug)]
enum IncrementerMessage {
    Increment,
    Ack,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum IncrementerRole {
    Incrementer,
    Forwarder,
    Counter,
}

type IncrementerEffector = Effector<IncrementerMessage, IncrementerRole>;
type IncrementerProtocol = Protocol<IncrementerMessage, IncrementerRole>;

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

impl From<CounterRole> for IncrementerRole {
    fn from(c: CounterRole) -> IncrementerRole {
        match c {
            CounterRole::Incrementer => {
                IncrementerRole::Incrementer
            },
            CounterRole::Forwarder => {
                IncrementerRole::Forwarder
            },
            CounterRole::Counter => IncrementerRole::Counter,
        }
    }
}

struct IncrementerLobe {
    effector: Option<IncrementerEffector>,

    output: Option<Handle>,
}

impl IncrementerLobe {
    fn new() -> Self {
        Self {
            effector: None,
            output: None,
        }
    }

    fn effector(&self) -> &IncrementerEffector {
        self.effector.as_ref().unwrap()
    }
}

impl Lobe for IncrementerLobe {
    type Message = IncrementerMessage;
    type Role = IncrementerRole;

    fn update(mut self, msg: IncrementerProtocol) -> Result<Self> {
        match msg {
            Protocol::Init(effector) => {
                println!("incrementer: {}", effector.this_lobe());
                self.effector = Some(effector);
            },
            Protocol::AddOutput(output, role) => {
                println!(
                    "incrementer output {} {:#?}", output, role
                );

                assert!(
                    role == IncrementerRole::Incrementer
                    || role == IncrementerRole::Forwarder
                );

                self.output = Some(output);
            },

            Protocol::Start => {
                if let Some(output) = self.output {
                    self.effector().send(
                        output, IncrementerMessage::Increment
                    );
                }
                else {
                    self.effector().stop();
                }
            },

            Protocol::Message(src, IncrementerMessage::Ack) => {
                assert_eq!(src, self.output.unwrap());
                println!("ACK");

                self.effector().send(
                    self.output.unwrap(), IncrementerMessage::Increment
                );
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

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum CounterRole {
    Incrementer,
    Forwarder,
    Counter,
}

type CounterEffector = Effector<CounterMessage, CounterRole>;
type CounterProtocol = Protocol<CounterMessage, CounterRole>;

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

impl From<IncrementerRole> for CounterRole {
    fn from(role: IncrementerRole) -> CounterRole {
        match role {
            IncrementerRole::Incrementer => {
                CounterRole::Incrementer
            },
            IncrementerRole::Forwarder => {
                CounterRole::Forwarder
            },
            IncrementerRole::Counter => {
                CounterRole::Counter
            },
        }
    }
}

struct CounterLobe {
    effector: Option<CounterEffector>,

    input: Option<Handle>,

    counter: u32
}

impl CounterLobe {
    fn new() -> Self {
        Self {
            effector: None,
            input: None,
            counter: 0
        }
    }

    fn effector(&self) -> &CounterEffector {
        self.effector.as_ref().unwrap()
    }
}

impl Lobe for CounterLobe {
    type Message = CounterMessage;
    type Role = CounterRole;

    fn update(mut self, msg: CounterProtocol) -> Result<Self>
    {
        match msg {
            Protocol::Init(effector) => {
                println!("counter: {}", effector.this_lobe());
                self.effector = Some(effector);
            },
            Protocol::AddInput(input, role) => {
                println!("counter input {} {:#?}", input, role);

                assert!(
                    role == CounterRole::Incrementer
                    || role == CounterRole::Forwarder
                );
                self.input = Some(input);
            },

            Protocol::Message(src, CounterMessage::BumpCounter) => {
                assert_eq!(src, self.input.unwrap());

                if self.counter < 5 {
                    println!("counter increment");

                    self.counter += 1;
                    self.effector().send(
                        self.input.unwrap(), CounterMessage::Ack
                    );
                }
                else {
                    println!("stop");
                    self.effector().stop();
                }
            },

            _ => (),
        }

        Ok(self)
    }
}

struct ForwarderLobe {
    effector: Option<Effector<CounterMessage, CounterRole>>,

    input: Option<Handle>,
    output: Option<Handle>,
}

impl ForwarderLobe {
    fn new() -> Self {
        Self { effector: None, input: None, output: None }
    }

    fn effector(&self) -> &Effector<CounterMessage, CounterRole> {
        self.effector.as_ref().unwrap()
    }
}

impl Lobe for ForwarderLobe {
    type Message = CounterMessage;
    type Role = CounterRole;

    fn update(mut self, msg: Protocol<Self::Message, Self::Role>)
        -> Result<Self>
    {
        match msg {
            Protocol::Init(effector) => {
                println!("forwarder: {}", effector.this_lobe());
                self.effector = Some(effector);
            },
            Protocol::AddInput(input, role) => {
                assert!(
                    role == CounterRole::Incrementer
                    || role == CounterRole::Forwarder
                );

                println!("forwarder input: {}", input);
                self.input = Some(input);
            },
            Protocol::AddOutput(output, role) => {
                assert!(
                    role == CounterRole::Counter
                    || role == CounterRole::Forwarder
                );

                println!("forwarder output: {}", output);
                self.output = Some(output);
            },

            Protocol::Message(src, msg) => {
                if src == self.input.unwrap() {
                    println!(
                        "forwarding input {:#?} through {}",
                        msg,
                        self.effector().this_lobe()
                    );

                    self.effector().send(self.output.unwrap(), msg);
                }
                else if src == self.output.unwrap() {
                    println!(
                        "forwarding output {:#?} through {}",
                        msg,
                        self.effector().this_lobe()
                    );

                    self.effector().send(self.input.unwrap(), msg);
                }
            },

            _ => ()
        }

        Ok(self)
    }
}

#[test]
fn test_cortex() {
    let mut cortex = Cortex::new(IncrementerLobe::new());

    let counter = cortex.add_lobe(CounterLobe::new());

    let main = cortex.get_main_handle();
    println!("cortex {}", main);
    cortex.connect(main, counter, IncrementerRole::Incrementer);

    run(cortex).unwrap();
}

#[test]
fn test_sub_cortex() {
    let mut counter_cortex = Cortex::new(ForwarderLobe::new());

    let forwarder = counter_cortex.get_main_handle();
    let counter = counter_cortex.add_lobe(CounterLobe::new());

    counter_cortex.connect(forwarder, counter, CounterRole::Forwarder);

    let mut inc_cortex = Cortex::new(IncrementerLobe::new());

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

    fn update(self, msg: IncrementerProtocol)
        -> Result<Self>
    {
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

    fn update(self, _: IncrementerProtocol)
        -> Result<Self>
    {
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
