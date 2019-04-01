use bam::{
	client::Client,
	config::Config,
	connection,
	json::{
		self, Frame, JsonFrameCodec, JsonFrameHandler, JsonResponseSource, OutgoingRequest,
		Response, ValidatedIncomingRequest,
	},
	shutdown_handle::ShutdownHandle,
};
use futures::{Future, Stream};
use memsocket::{self, UnboundedSocket};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncWrite, ReadHalf, WriteHalf};
use tokio_codec::{FramedRead, LinesCodec};

pub struct Alice {
	read: Arc<Mutex<FramedRead<ReadHalf<UnboundedSocket>, LinesCodec>>>,
	write: Arc<Mutex<WriteHalf<UnboundedSocket>>>,
}

impl Alice {
	pub fn send_with_newline<S: Into<String>>(
		&self,
		msg: S,
	) -> impl Future<Item = (), Error = ::std::io::Error> {
		let msg = msg.into();

		let write = self.write.clone();

		let send_future = ::futures::future::poll_fn(move || {
			let mut write = write.lock().unwrap();

			let msg = format!("{}\n", msg);

			write.poll_write(msg.as_str().as_bytes())
		});

		send_future.map(|_| ())
	}

	pub fn receive(&self) -> impl Future<Item = Option<String>, Error = ::std::io::Error> {
		let read = self.read.clone();

		::futures::future::poll_fn(move || {
			let mut read = read.lock().unwrap();

			read.poll()
		})
	}

	#[allow(dead_code)]
	pub fn send_without_newline(
		&self,
		msg: &'static str,
	) -> impl Future<Item = Option<String>, Error = ::std::io::Error> {
		let write = self.write.clone();
		let read = self.read.clone();

		let send_future = ::futures::future::poll_fn(move || {
			let mut write = write.lock().unwrap();

			write.poll_write(msg.as_bytes())
		});

		let receive_future = ::futures::future::poll_fn(move || {
			let mut read = read.lock().unwrap();

			read.poll()
		});

		send_future.and_then(move |s| {
			debug_assert_eq!(s, msg.len(), "Did not send all bytes!");

			receive_future
		})
	}
}

pub struct Bob {
	pub _alice: Client<Frame, OutgoingRequest, Response>,
	pub _shutdown_handle: ShutdownHandle,
}

pub fn create(
	config: Config<ValidatedIncomingRequest, Response>,
) -> (
	Alice,
	impl Future<Item = (), Error = connection::ClosedReason<json::Error>>,
	Client<Frame, OutgoingRequest, Response>,
) {
	let (alice, bob) = memsocket::unbounded();

	let response_source = Arc::new(Mutex::new(JsonResponseSource::default()));
	let incoming_frames = JsonFrameHandler::create(config, Arc::clone(&response_source));
	let (alice_client, outgoing_frames) = Client::create(response_source);

	let bob_server = connection::new(
		JsonFrameCodec::default(),
		bob,
		incoming_frames,
		outgoing_frames,
	);

	let (read, write) = alice.split();

	let alice = Alice {
		read: Arc::new(Mutex::new(FramedRead::new(read, LinesCodec::new()))),
		write: Arc::new(Mutex::new(write)),
	};

	(alice, bob_server, alice_client)
}
