#![warn(missing_docs)]

//! Cortical - general purpose reactive lobe networks

#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate tokio;
extern crate tokio_core;
extern crate uuid;

use std::cell::{ RefCell };
use std::collections::{ HashMap };
use std::marker::PhantomData;
use std::mem;
use std::rc::Rc;

use futures::prelude::*;
use futures::sync::{ mpsc };
use tokio_core::reactor;
use uuid::Uuid;

/// cortical error
error_chain! {
    foreign_links {
        Io(std::io::Error) #[doc = "glue for io::Error"];
    }
    errors {
        /// a lobe returned an error when called into
        LobeError {
            description("an error occurred while calling into a lobe"),
            display("an error occurred while calling into a lobe")
        }
    }
}

/// handle to a lobe within the cortex
pub type Handle = Uuid;

/// a set of protocol messages to be relayed throughout the network
///
/// wraps a user-defined message within the cortex protocol.
/// 1. a lobe is always updated with Init first.
/// 2. as the cortex is built, the lobe will be updated with any inputs or
///     outputs specified using AddInput and AddOutput.
/// 3. when the cortex is ready to begin execution, every lobe is updated with
///     Start
/// 4. any messages sent between lobes will come through Message
/// 5. when a lobe determines that the cortex should stop, it can issue Stop
///     and the cortex will exit its event loop.
pub enum Protocol<M> {
    /// initializes a lobe with an effector to use
    Init(Effector<M>),
    /// notifies lobe that it has an input specified by Handle
    AddInput(Handle),
    /// notifies lobe that it has an output specified by Handle
    AddOutput(Handle),

    /// notifies lobe that cortex has begun execution
    Start,

    /// internal use only - used to track source and destination of message
    Payload(Handle, Handle, M),

    /// updates the lobe with a user-defined message from source lobe Handle
    Message(Handle, M),

    /// tells the cortex to stop executing
    Stop,
}

impl<M> Protocol<M> {
    fn convert_protocol<T>(msg: Protocol<T>) -> Self
        where
            M: From<T> + Into<T> + 'static,
            T: From<M> + Into<M> + 'static,
    {
        match msg {
            Protocol::Init(effector) => {
                let sender = effector.sender;

                Protocol::Init(
                    Effector {
                        handle: effector.handle,
                        sender: Rc::from(
                            move |effector: &reactor::Handle, msg: Protocol<M>| sender(
                                effector,
                                Protocol::<T>::convert_protocol(
                                    msg
                                )
                            )
                        ),
                        reactor: effector.reactor,
                    }
                )
            },

            Protocol::AddInput(input) => Protocol::AddInput(input),
            Protocol::AddOutput(output) => Protocol::AddOutput(
                output
            ),

            Protocol::Start => Protocol::Start,

            Protocol::Payload(src, dest, msg) => Protocol::Payload(
                src, dest, msg.into()
            ),
            Protocol::Message(src, msg) => Protocol::Message(
                src, msg.into()
            ),

            Protocol::Stop => Protocol::Stop,
        }
    }
}

/// the effector is a lobe's method of communicating between other lobes
///
/// the effector can send a message to any destination, provided you have its
/// handle. it will route these messages asynchronously to their destination,
/// so communication can be tricky, however, this is truly the best way I've
/// found to compose efficient, scalable systems.
#[derive(Clone)]
pub struct Effector<M> {
    handle:     Handle,
    sender:     Rc<Fn(&reactor::Handle, Protocol<M>)>,
    reactor:    reactor::Handle,
}

impl<M> Effector<M> where M: 'static {
    /// get the Handle associated with the lobe that owns this effector
    pub fn handle(&self) -> Handle {
        self.handle
    }

    /// send a message to dest lobe
    pub fn send(&self, dest: Handle, msg: M) {
        self.send_cortex_message(
            Protocol::Payload(self.handle(), dest, msg)
        );
    }

    /// stop the cortex
    pub fn stop(&self) {
        self.send_cortex_message(Protocol::Stop);
    }

    /// spawn a future on the reactor
    pub fn spawn<F>(&self, future: F) where
        F: Future<Item=(), Error=()> + 'static
    {
        self.reactor.spawn(future);
    }

    fn send_cortex_message(&self, msg: Protocol<M>) {
        (*self.sender)(&self.reactor, msg);
    }
}


/// defines an interface for a lobe of any type
///
/// generic across the user-defined message to be passed between lobes
pub trait Lobe: Sized {
    /// user-defined message to be passed between lobes
    type Message;
    /// apply any changes to the lobe's state as a result of _msg
    fn update(self, _msg: Protocol<Self::Message>) -> Self {
        self
    }
}

trait Node<M> {
    fn update(&mut self, msg: Protocol<M>);
}

struct LobeWrapper<L, M>(Option<L>, PhantomData<M>);

impl<L, M> LobeWrapper<L, M> where L: Lobe<Message=M> {
    fn new(lobe: L) -> Self {
        LobeWrapper::<L, M>(Some(lobe), PhantomData::default())
    }
}

impl<L, I, O> Node<O> for LobeWrapper<L, I> where
    L: Lobe<Message=I>,
    I: From<O> + Into<O> + 'static,
    O: From<I> + Into<I> + 'static
{
    fn update(&mut self, msg: Protocol<O>) {
        let lobe = mem::replace(&mut self.0, None)
            .unwrap()
            .update(Protocol::<I>::convert_protocol(msg))
        ;

        self.0 = Some(lobe);
    }
}

struct CortexNodePool<M> {
    input_hdl:      Handle,
    output_hdl:     Handle,

    input:          Box<Node<M>>,
    output:         Box<Node<M>>,

    misc:           HashMap<Handle, Box<Node<M>>>,
}

/// a special lobe designed to contain a network of interconnected lobes
///
/// the cortex is created with one input lobe and one output lobe. these lobes
/// are special in that they are the only lobes within the cortex that are
/// allowed to communicate or connect to the outside world. the input node can
/// act as an entry point for the network, providing essential external data
/// while keeping implementation-specific data and lobes hidden. upon receiving
/// an update, the output node has the opportunity to communicate these updates
/// with external lobes.
///
/// the intent is to allow cortices to be hierarchical and potentially contain
/// any number of nested lobe networks. in order to do this, the cortex
/// isolates a group of messages from the larger whole. this is essential for
/// extensibility and maintainability.
///
/// any cortex can be plugged into any other cortex provided their messages can
/// convert between each other using From and Into
pub struct Cortex<M> {
    effector:       Option<Effector<M>>,

    input_hdl:      Handle,
    output_hdl:     Handle,
    connections:    Vec<(Handle, Handle)>,

    nodes:          Rc<RefCell<CortexNodePool<M>>>,
}

impl<M> Cortex<M> {
    /// create a new cortex with input and output lobes
    pub fn new<I, O, IM, OM>(input: I, output: O) -> Self where
        M: From<IM> + Into<IM> + From<OM> + Into<OM> + 'static,

        I: Lobe<Message=IM> + 'static,
        O: Lobe<Message=OM> + 'static,

        IM: From<M> + Into<M> + 'static,
        OM: From<M> + Into<M> + 'static,
    {
        let input_hdl = Handle::new_v4();
        let output_hdl = Handle::new_v4();

        Self {
            effector: None,

            input_hdl: input_hdl,
            output_hdl: output_hdl,
            connections: vec![ ],

            nodes: Rc::from(
                RefCell::new(
                    CortexNodePool::<M> {
                        input_hdl: input_hdl,
                        output_hdl: output_hdl,

                        input: Box::new(LobeWrapper::new(input)),
                        output: Box::new(LobeWrapper::new(output)),

                        misc: HashMap::new()
                    }
                )
            )
        }
    }


    /// add a new lobe to the cortex and initialize it
    ///
    /// as long as the lobe's message type can convert Into and From the
    /// cortex's message type, it can be added to the cortex and can
    /// communicate with any lobes that do the same.
    pub fn add_lobe<L, T>(&mut self, lobe: L) -> Handle where
        L: Lobe<Message=T> + 'static,
        M: From<T> + Into<T> + 'static,
        T: From<M> + Into<M> + 'static,
    {
        let node = Box::new(LobeWrapper::new(lobe));
        let handle = Handle::new_v4();

        (*self.nodes).borrow_mut().misc.insert(handle, node);

        handle
    }

    /// connect input to output and update them accordingly
    pub fn connect(&mut self, input: Handle, output: Handle) {
        self.connections.push((input, output));
    }

    /// get the input lobe's handle
    pub fn get_input(&self) -> Handle {
        self.input_hdl
    }

    /// get the output lobe's handle
    pub fn get_output(&self) -> Handle {
        self.output_hdl
    }

    fn update_node(&self, hdl: Handle, msg: Protocol<M>) {
        let mut nodes = (*self.nodes).borrow_mut();

        if hdl == nodes.input_hdl {
            nodes.input.update(msg);
        }
        else if hdl == nodes.output_hdl {
            nodes.output.update(msg);
        }
        else {
            nodes.misc.get_mut(&hdl).unwrap().update(msg);
        }
    }

    fn init<T>(mut self, effector: Effector<T>) -> Self where
        M: From<T> + Into<T> + 'static,
        T: From<M> + Into<M> + 'static,
    {
        let cortex_hdl = effector.handle;

        let (queue_tx, queue_rx) = mpsc::channel(100);

        self.effector = Some(
            Effector {
                handle: cortex_hdl.clone(),
                sender: Rc::from(
                    move |r: &reactor::Handle, msg: Protocol<M>| r.spawn(
                        queue_tx.clone().send(msg)
                           .then(
                               |result| match result {
                                   Ok(_) => Ok(()),
                                   Err(_) => Ok(())
                               }
                           )
                    )
                ),
                reactor: effector.reactor,
            }
        );

        let sender = self.effector
            .as_ref()
            .unwrap()
            .sender
            .clone()
        ;
        let reactor = self.effector
            .as_ref()
            .unwrap()
            .reactor
            .clone()
        ;

        let input_hdl = self.input_hdl;
        let output_hdl = self.output_hdl;

        self.update_node(
            input_hdl,
            Protocol::Init(
                Effector {
                    handle: input_hdl,
                    sender: sender.clone(),
                    reactor: reactor.clone(),
                }
            )
        );
        self.update_node(
            output_hdl,
            Protocol::Init(
                Effector {
                    handle: output_hdl,
                    sender: sender.clone(),
                    reactor: reactor.clone(),
                }
            )
        );

        for node in (*self.nodes).borrow_mut().misc.values_mut() {
            node.update(
                Protocol::Init(
                    Effector {
                        handle: Handle::new_v4(),
                        sender: sender.clone(),
                        reactor: reactor.clone(),
                    }
                )
            );
        }

        for &(input, output) in &self.connections {
            self.update_node(input, Protocol::AddOutput(output));
            self.update_node(output, Protocol::AddInput(input));
        }

        let external_sender = effector.sender;
        let nodes = Rc::clone(&self.nodes);
        let forward_reactor = reactor.clone();

        let stream_future = queue_rx.for_each(
            move |msg| {
                Self::forward(
                    cortex_hdl,
                    &mut (*nodes).borrow_mut(),
                    &*external_sender,
                    &forward_reactor,
                    msg
                );

                Ok(())
            }
        );

        reactor.spawn(stream_future);

        self
    }

    fn start(self) -> Self {
        {
            let mut nodes = (*self.nodes).borrow_mut();

            nodes.input.update(Protocol::Start);
            nodes.output.update(Protocol::Start);

            for node in nodes.misc.values_mut() {
                node.update(Protocol::Start);
            }
        }

        self
    }

    fn add_input(self, input: Handle) -> Self {
        (*self.nodes).borrow_mut().input.update(Protocol::AddInput(input));

        self
    }

    fn add_output(self, output: Handle) -> Self {
        (*self.nodes).borrow_mut().output.update(Protocol::AddOutput(output));

        self
    }

    fn forward<T>(
        cortex: Handle,
        nodes: &mut CortexNodePool<M>,
        sender: &Fn(&reactor::Handle, Protocol<T>),
        reactor: &reactor::Handle,
        msg: Protocol<M>
    ) where
        M: From<T> + Into<T> + 'static,
        T: From<M> + Into<M> + 'static,
    {
        match msg {
            Protocol::Payload(src, dest, msg) => {
                let actual_src = {
                    // check if src is output or input
                    if src == nodes.output_hdl || src == nodes.input_hdl {
                        // if src is a special node, then it becomes tricky.
                        // these are allowed to send to both internal and
                        // external nodes, so the question becomes whether or
                        // not to advertise itself as the node or the cortex

                        if dest == nodes.input_hdl
                            || dest == nodes.output_hdl
                            || nodes.misc.contains_key(&dest)
                        {
                            // internal node - use src
                            src
                        }
                        else {
                            // external node - use cortex hdl
                            cortex
                        }
                    }
                    else {
                        src
                    }
                };

                if dest == nodes.input_hdl {
                    nodes.input.update(Protocol::Message(actual_src, msg));
                }
                else if dest == nodes.output_hdl {
                    nodes.output.update(Protocol::Message(actual_src, msg));
                }
                else if let Some(ref mut node) = nodes.misc.get_mut(&dest) {
                    // send to internal node
                    node.update(Protocol::Message(actual_src, msg));
                }
                else {
                    // send to external node
                    sender(
                        reactor,
                        Protocol::<T>::convert_protocol(
                            Protocol::Payload(actual_src, dest, msg)
                        )
                    );
                }
            },

            Protocol::Stop => sender(reactor, Protocol::Stop),

            _ => unimplemented!()
        }
    }
}

impl<M> Lobe for Cortex<M> where M: 'static
{
    type Message = M;

    fn update(self, msg: Protocol<M>) -> Self {
        match msg {
            Protocol::Init(effector) => self.init(effector),
            Protocol::AddInput(input) => self.add_input(input),
            Protocol::AddOutput(output) => self.add_output(output),

            Protocol::Start => self.start(),
            Protocol::Message(src, msg) => {
                self.update_node(
                    self.input_hdl,
                    Protocol::Message(src, msg)
                );

                self
            },

            _ => unreachable!(),
        }
    }
}

/// spin up an event loop and run the provided lobe
pub fn run<T, M>(lobe: T) -> Result<()> where
    T: Lobe<Message=M>,
    M: 'static,
{
    let (queue_tx, queue_rx) = mpsc::channel(100);
    let mut core = reactor::Core::new()?;

    let handle = Handle::new_v4();
    let reactor = core.handle();

    // Rc to keep it alive, RefCell to mutate it in the event loop
    let mut node = LobeWrapper::new(lobe);

    reactor.clone().spawn(
        queue_tx.clone()
            .send(
                Protocol::Init(
                    Effector {
                        handle: handle,
                        sender: Rc::from(
                            move |r: &reactor::Handle, msg| r.spawn(
                                 queue_tx.clone()
                                    .send(msg)
                                    .then(|_| Ok(()))
                            )
                        ),
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

    let stream_future = queue_rx.take_while(
        |msg| match *msg {
            Protocol::Stop => {
                println!("ended cleanly");
                Ok(false)
            },
            _ => Ok(true)
        }
    ).for_each(
        move |msg| {
            match msg {
                Protocol::Init(effector) => node.update(
                    Protocol::Init(effector)
                ),
                Protocol::AddInput(input) => node.update(
                    Protocol::AddInput(input)
                ),
                Protocol::AddOutput(output) => node.update(
                    Protocol::AddOutput(output)
                ),

                Protocol::Start => node.update(Protocol::Start),

                Protocol::Payload(src, dest, msg) => {
                    // messages should only be sent to our main lobe
                    assert_eq!(dest, handle);

                    node.update(Protocol::Message(src, msg));
                },

                _ => unreachable!(),
            }

            Ok(())
        }
    );

    core.run(
        stream_future
            .map(|_| ())
            .map_err(|_| -> Error {
                ErrorKind::Msg("whoops!".into()).into()
            })
    )?;

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

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

    struct IncrementerLobe {
        effector: Option<Effector<IncrementerMessage>>,

        output: Option<Handle>,
    }

    impl IncrementerLobe {
        fn new() -> Self {
            Self {
                effector: None,
                output: None,
            }
        }

        fn effector(&self) -> &Effector<IncrementerMessage> {
            self.effector.as_ref().unwrap()
        }
    }

    impl Lobe for IncrementerLobe {
        type Message = IncrementerMessage;

        fn update(mut self, msg: Protocol<Self::Message>) -> Self {
            match msg {
                Protocol::Init(effector) => {
                    println!("incrementer: {}", effector.handle());
                    self.effector = Some(effector);
                },
                Protocol::AddOutput(output) => {
                    println!("incrementer output: {}", output);
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

            self
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

    struct CounterLobe {
        effector: Option<Effector<CounterMessage>>,

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

        fn effector(&self) -> &Effector<CounterMessage> {
            self.effector.as_ref().unwrap()
        }
    }

    impl Lobe for CounterLobe {
        type Message = CounterMessage;

        fn update(mut self, msg: Protocol<Self::Message>) -> Self {
            match msg {
                Protocol::Init(effector) => {
                    println!("counter: {}", effector.handle());
                    self.effector = Some(effector);
                },
                Protocol::AddInput(input) => {
                    println!("counter input: {}", input);
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

            self
        }
    }

    struct ForwarderLobe {
        effector: Option<Effector<CounterMessage>>,

        input: Option<Handle>,
        output: Option<Handle>,
    }

    impl ForwarderLobe {
        fn new() -> Self {
            Self { effector: None, input: None, output: None }
        }

        fn effector(&self) -> &Effector<CounterMessage> {
            self.effector.as_ref().unwrap()
        }
    }

    impl Lobe for ForwarderLobe {
        type Message = CounterMessage;

        fn update(mut self, msg: Protocol<Self::Message>) -> Self {
            match msg {
                Protocol::Init(effector) => {
                    println!("forwarder: {}", effector.handle());
                    self.effector = Some(effector);
                },
                Protocol::AddInput(input) => {
                    println!("forwarder input: {}", input);
                    self.input = Some(input);
                },
                Protocol::AddOutput(output) => {
                    println!("forwarder output: {}", output);
                    self.output = Some(output);
                },

                Protocol::Message(src, msg) => {
                    if src == self.input.unwrap() {
                        println!(
                            "forwarding input {:#?} through {}",
                            msg,
                            self.effector().handle()
                        );

                        self.effector().send(self.output.unwrap(), msg);
                    }
                    else if src == self.output.unwrap() {
                        println!(
                            "forwarding output {:#?} through {}",
                            msg,
                            self.effector().handle()
                        );

                        self.effector().send(self.input.unwrap(), msg);
                    }
                },

                _ => ()
            }

            self
        }
    }

    #[test]
    fn test_cortex() {
        let mut cortex = Cortex::<IncrementerMessage>::new(
            IncrementerLobe::new(), CounterLobe::new()
        );

        let input = cortex.get_input();
        let output = cortex.get_output();
        cortex.connect(input, output);

        run(cortex).unwrap();
    }

    #[test]
    fn test_sub_cortex() {
        let mut counter_cortex: Cortex<CounterMessage> = Cortex::new(
            ForwarderLobe::new(), CounterLobe::new()
        );

        let counter_input = counter_cortex.get_input();
        let counter_output = counter_cortex.get_output();
        counter_cortex.connect(counter_input, counter_output);

        let mut inc_cortex: Cortex<IncrementerMessage> = Cortex::new(
            IncrementerLobe::new(), counter_cortex
        );

        let inc_input = inc_cortex.get_input();
        let inc_output = inc_cortex.get_output();
        inc_cortex.connect(inc_input, inc_output);

        run(inc_cortex).unwrap();
    }
}
