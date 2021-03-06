use crate::libp2p_bam::{protocol::BamProtocol, BamStream};
use bam::{
    json::{
        Frame, FrameType, Header, JsonFrameCodec, OutgoingRequest, Response,
        UnknownMandatoryHeaders, UnvalidatedIncomingRequest, ValidatedIncomingRequest,
    },
    IntoFrame, Status,
};
use derivative::Derivative;
use futures::{
    sink::Sink,
    stream::Stream,
    sync::oneshot::{self, Canceled},
    task::Task,
    Async, AsyncSink, Future, Poll,
};
use libp2p::core::{
    protocols_handler::{
        KeepAlive, ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr,
        SubstreamProtocol,
    },
    upgrade::Negotiated,
};
use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
};
use tokio::{
    codec::Framed,
    prelude::{AsyncRead, AsyncWrite},
};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct BamHandler<TSubstream> {
    #[derivative(Debug = "ignore")]
    substreams: Vec<SubstreamState<TSubstream>>,
    #[derivative(Debug = "ignore")]
    current_task: Option<Task>,

    known_headers: HashMap<String, HashSet<String>>,
}

#[derive(strum_macros::Display)]
#[allow(missing_debug_implementations)]
/// State of an active substream, opened either by us or by the remote.
enum SubstreamState<TSubstream> {
    /// We haven't started opening the outgoing substream yet.
    OutPendingOpen { req: PendingOutgoingRequest },
    /// Waiting to send a message to the remote.
    OutPendingSend {
        msg: Frame,
        response_sender: oneshot::Sender<Response>,
        stream: BamStream<TSubstream>,
    },
    /// Waiting to flush the substream so that the data arrives to the remote.
    OutPendingFlush {
        response_sender: oneshot::Sender<Response>,
        stream: BamStream<TSubstream>,
    },
    /// Waiting for the answer to our message
    OutWaitingAnswer {
        response_sender: oneshot::Sender<Response>,
        stream: BamStream<TSubstream>,
    },
    /// Waiting for a request from the remote.
    InWaitingMessage { stream: BamStream<TSubstream> },
    /// Waiting for the user to send the response back to us.
    InWaitingUser {
        response_receiver: oneshot::Receiver<Response>,
        stream: BamStream<TSubstream>,
    },
    /// Waiting to send an answer back to the remote.
    InPendingSend {
        msg: Frame,
        stream: BamStream<TSubstream>,
    },
    /// Waiting to flush an answer back to the remote.
    InPendingFlush { stream: BamStream<TSubstream> },
    /// The substream is being closed.
    Closing { stream: BamStream<TSubstream> },
}

#[allow(missing_debug_implementations)]
struct Advanced<TSubstream> {
    /// The optional new state we transitioned to
    new_state: Option<SubstreamState<TSubstream>>,
    /// The optional event we generated as part of the transition
    event: Option<BamHandlerEvent>,
}

#[derive(Debug)]
pub enum Error {
    Stream(bam::json::Error),
    DroppedResponseSender(Canceled),
}

impl From<Canceled> for Error {
    fn from(e: Canceled) -> Self {
        Error::DroppedResponseSender(e)
    }
}

impl From<bam::json::Error> for Error {
    fn from(e: bam::json::Error) -> Self {
        Error::Stream(e)
    }
}

impl<TSubstream> Advanced<TSubstream> {
    fn transition_to(new_state: SubstreamState<TSubstream>) -> Self {
        Self {
            new_state: Some(new_state),
            event: None,
        }
    }

    fn emit_event(event: BamHandlerEvent) -> Self {
        Self {
            new_state: None,
            event: Some(event),
        }
    }

    fn error<E: Into<Error>>(stream: BamStream<TSubstream>, error: E) -> Self {
        let error = error.into();

        Self {
            new_state: Some(SubstreamState::Closing { stream }),
            event: Some(ProtocolsHandlerEvent::Custom(InnerEvent::Error { error })),
        }
    }

    fn end() -> Self {
        Self {
            new_state: None,
            event: None,
        }
    }

    fn closed_unexpectedly(stream: BamStream<TSubstream>) -> Self {
        Self {
            new_state: Some(SubstreamState::Closing { stream }),
            event: Some(ProtocolsHandlerEvent::Custom(InnerEvent::UnexpectedEOF)),
        }
    }
}

impl<TSubstream: AsyncRead + AsyncWrite> SubstreamState<TSubstream> {
    fn advance(self, known_headers: &HashMap<String, HashSet<String>>) -> Advanced<TSubstream> {
        use self::SubstreamState::*;
        match self {
            OutPendingOpen { req } => {
                Advanced::emit_event(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(BamProtocol {}),
                    info: req,
                })
            }
            OutPendingSend {
                msg,
                response_sender,
                mut stream,
            } => match stream.start_send(msg) {
                Ok(AsyncSink::Ready) => OutPendingFlush {
                    response_sender,
                    stream,
                }
                .advance(known_headers),
                Ok(AsyncSink::NotReady(msg)) => Advanced::transition_to(OutPendingSend {
                    msg,
                    response_sender,
                    stream,
                }),
                Err(error) => Advanced::error(stream, error),
            },
            OutPendingFlush {
                response_sender,
                mut stream,
            } => match stream.poll_complete() {
                Ok(Async::Ready(_)) => Advanced::transition_to(OutWaitingAnswer {
                    response_sender,
                    stream,
                }),
                Ok(Async::NotReady) => Advanced::transition_to(OutPendingFlush {
                    response_sender,
                    stream,
                }),
                Err(error) => Advanced::error(stream, error),
            },
            OutWaitingAnswer {
                response_sender,
                mut stream,
            } => match stream.poll() {
                Ok(Async::Ready(Some(frame))) => {
                    let expected_type = FrameType::Response;
                    if frame.frame_type != expected_type {
                        return Advanced {
                            new_state: Some(Closing { stream }),
                            event: Some(ProtocolsHandlerEvent::Custom(
                                InnerEvent::UnexpectedFrameType {
                                    bad_frame: frame,
                                    expected_type,
                                },
                            )),
                        };
                    }

                    let event = serde_json::from_value(frame.payload)
                        .map(|response| {
                            InnerEvent::IncomingResponse(PendingIncomingResponse {
                                response,
                                channel: response_sender,
                            })
                        })
                        .unwrap_or_else(|deser_error| {
                            log::error!(
                                target: "bam",
                                "payload of frame is not a well-formed RESPONSE: {:?}",
                                deser_error
                            );

                            InnerEvent::BadIncomingResponse
                        });

                    Advanced {
                        new_state: Some(Closing { stream }),
                        event: Some(ProtocolsHandlerEvent::Custom(event)),
                    }
                }
                Ok(Async::Ready(None)) => Advanced::closed_unexpectedly(stream),
                Ok(Async::NotReady) => Advanced::transition_to(OutWaitingAnswer {
                    response_sender,
                    stream,
                }),
                Err(error) => Advanced::error(stream, error),
            },
            InWaitingMessage { mut stream } => match stream.poll() {
                Ok(Async::Ready(Some(frame))) => {
                    let expected_type = FrameType::Request;
                    if frame.frame_type != expected_type {
                        return Advanced {
                            new_state: Some(Closing { stream }),
                            event: Some(ProtocolsHandlerEvent::Custom(
                                InnerEvent::UnexpectedFrameType {
                                    bad_frame: frame,
                                    expected_type,
                                },
                            )),
                        };
                    }

                    let request =
                        serde_json::from_value::<UnvalidatedIncomingRequest>(frame.payload)
                            .map_err(malformed_request_response)
                            .and_then(|request| {
                                known_headers
                                    .get(request.request_type())
                                    .ok_or_else(|| {
                                        unknown_request_type_response(request.request_type())
                                    })
                                    .and_then(|known_headers| {
                                        request
                                            .ensure_no_unknown_mandatory_headers(known_headers)
                                            .map_err(unknown_mandatory_headers_response)
                                    })
                            });

                    let (sender, receiver) = oneshot::channel();

                    Advanced {
                        new_state: Some(InWaitingUser {
                            response_receiver: receiver,
                            stream,
                        }),
                        event: Some(ProtocolsHandlerEvent::Custom(match request {
                            Ok(request) => InnerEvent::IncomingRequest(PendingIncomingRequest {
                                request,
                                channel: sender,
                            }),
                            Err(response) => InnerEvent::BadIncomingRequest(
                                AutomaticallyGeneratedErrorResponse {
                                    response,
                                    channel: sender,
                                },
                            ),
                        })),
                    }
                }
                Ok(Async::Ready(None)) => Advanced::closed_unexpectedly(stream),
                Ok(Async::NotReady) => Advanced::transition_to(InWaitingMessage { stream }),
                Err(error) => Advanced::error(stream, error),
            },
            InWaitingUser {
                mut response_receiver,
                stream,
            } => match response_receiver.poll() {
                Ok(Async::Ready(response)) => InPendingSend {
                    msg: response.into_frame(),
                    stream,
                }
                .advance(known_headers),
                Ok(Async::NotReady) => Advanced::transition_to(InWaitingUser {
                    response_receiver,
                    stream,
                }),
                Err(error) => Advanced::error(stream, error),
            },
            InPendingSend { msg, mut stream } => match stream.start_send(msg) {
                Ok(AsyncSink::Ready) => InPendingFlush { stream }.advance(known_headers),
                Ok(AsyncSink::NotReady(msg)) => {
                    Advanced::transition_to(InPendingSend { msg, stream })
                }
                Err(error) => Advanced::error(stream, error),
            },
            InPendingFlush { mut stream } => match stream.poll_complete() {
                Ok(Async::Ready(_)) => Advanced::transition_to(Closing { stream }),
                Ok(Async::NotReady) => Advanced::transition_to(InPendingFlush { stream }),
                Err(error) => Advanced::error(stream, error),
            },

            Closing { mut stream } => match stream.close() {
                Ok(Async::Ready(_)) => Advanced::end(),
                Ok(Async::NotReady) => Advanced::transition_to(Closing { stream }),
                Err(error) => Advanced::error(stream, error),
            },
        }
    }
}

impl<TSubstream> BamHandler<TSubstream> {
    pub fn new(known_headers: HashMap<String, HashSet<String>>) -> Self {
        Self {
            known_headers,
            substreams: Vec::new(),
            current_task: None,
        }
    }
}

#[derive(Debug)]
pub struct PendingOutgoingRequest {
    pub request: OutgoingRequest,
    pub channel: oneshot::Sender<Response>,
}

#[derive(Debug)]
pub struct PendingIncomingRequest {
    pub request: ValidatedIncomingRequest,
    pub channel: oneshot::Sender<Response>,
}

#[derive(Debug)]
pub struct PendingIncomingResponse {
    pub response: Response,
    pub channel: oneshot::Sender<Response>,
}

#[derive(Debug)]
pub struct AutomaticallyGeneratedErrorResponse {
    pub response: Response,
    pub channel: oneshot::Sender<Response>,
}

#[derive(Debug)]
pub enum InnerEvent {
    IncomingRequest(PendingIncomingRequest),
    IncomingResponse(PendingIncomingResponse),
    BadIncomingRequest(AutomaticallyGeneratedErrorResponse),
    BadIncomingResponse,
    UnexpectedFrameType {
        bad_frame: Frame,
        expected_type: FrameType,
    },
    UnexpectedEOF,
    Error {
        error: Error,
    },
}

type BamHandlerEvent = ProtocolsHandlerEvent<BamProtocol, PendingOutgoingRequest, InnerEvent>;

impl<TSubstream: AsyncRead + AsyncWrite> ProtocolsHandler for BamHandler<TSubstream> {
    type InEvent = PendingOutgoingRequest;
    type OutEvent = InnerEvent;
    type Error = bam::json::Error;
    type Substream = TSubstream;
    type InboundProtocol = BamProtocol;
    type OutboundProtocol = BamProtocol;
    type OutboundOpenInfo = PendingOutgoingRequest;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(BamProtocol {})
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        stream: Framed<Negotiated<TSubstream>, JsonFrameCodec>,
    ) {
        self.substreams
            .push(SubstreamState::InWaitingMessage { stream });

        if let Some(task) = &self.current_task {
            task.notify()
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        stream: Framed<Negotiated<TSubstream>, JsonFrameCodec>,
        pending_incoming_request: Self::OutboundOpenInfo,
    ) {
        let PendingOutgoingRequest { request, channel } = pending_incoming_request;

        self.substreams.push(SubstreamState::OutPendingSend {
            msg: request.into_frame(),
            response_sender: channel,
            stream,
        });

        if let Some(task) = &self.current_task {
            task.notify()
        }
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        self.substreams
            .push(SubstreamState::OutPendingOpen { req: event });

        if let Some(task) = &self.current_task {
            task.notify()
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _info: Self::OutboundOpenInfo,
        _error: ProtocolsHandlerUpgrErr<Infallible>,
    ) {
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        KeepAlive::Yes
    }

    fn poll(&mut self) -> Poll<BamHandlerEvent, Self::Error> {
        log::debug!("polling {} substreams", self.substreams.len());

        // We remove each element from `substreams` one by one and add them back.
        for n in (0..self.substreams.len()).rev() {
            let substream_state = self.substreams.swap_remove(n);

            let log_message = format!("transition from {}", substream_state);

            let Advanced { new_state, event } = substream_state.advance(&self.known_headers);

            if let Some(new_state) = new_state {
                log::trace!(target: "sub-libp2p", "{} to {}", log_message, new_state);
                self.substreams.push(new_state);
            }

            if let Some(event) = event {
                log::trace!(target: "sub-libp2p", "emitting {:?}", event);
                return Ok(Async::Ready(event));
            }
        }

        self.current_task = Some(futures::task::current());

        Ok(Async::NotReady)
    }
}

fn malformed_request_response(error: serde_json::Error) -> Response {
    log::warn!(target: "sub-libp2p", "incoming request was malformed: {:?}", error);

    Response::new(Status::SE(0))
}

fn unknown_request_type_response(request_type: &str) -> Response {
    log::warn!(target: "sub-libp2p", "request type '{}' is unknown", request_type);

    Response::new(Status::SE(2))
}

fn unknown_mandatory_headers_response(unknown_headers: UnknownMandatoryHeaders) -> Response {
    Response::new(Status::SE(1)).with_header(
        "Unsupported-Headers",
        Header::with_value(unknown_headers)
            .expect("list of strings should serialize to serde_json::Value"),
    )
}
