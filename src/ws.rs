use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{SinkExt, Stream, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    tungstenite, tungstenite::client::IntoClientRequest, tungstenite::http::HeaderValue,
    tungstenite::Message, Connector, MaybeTlsStream, WebSocketStream,
};

use crate::{
    model::ws::{deserialize::DeEvent, LcuEvent, LcuSubscriptionType},
    utils::error::LcuWebsocketError,
    utils::process_info,
};

pub struct WSClient(WebSocketStream<MaybeTlsStream<TcpStream>>);

impl WSClient {
    pub async fn connect() -> Result<Self, LcuWebsocketError> {
        let (auth_token, port) = process_info::get_auth_info()
            .map_err(|e| LcuWebsocketError::LcuNotAvailable(e.to_string()))?;

        let cert = native_tls::Certificate::from_pem(include_bytes!("./riotgames.pem")).unwrap();
        let tls = native_tls::TlsConnector::builder()
            .add_root_certificate(cert)
            .build()
            .map_err(|_| LcuWebsocketError::AuthError)?;
        let connector = Connector::NativeTls(tls);

        let mut url = format!("wss://127.0.0.1:{port}")
            .into_client_request()
            .map_err(|_| LcuWebsocketError::AuthError)?;
        url.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(format!("Basic {auth_token}").as_str())
                .map_err(|_| LcuWebsocketError::AuthError)?,
        );

        let (ws_stream, _response) =
            tokio_tungstenite::connect_async_tls_with_config(url, None, Some(connector))
                .await
                .map_err(|e| LcuWebsocketError::Disconnected(e.to_string()))?;

        Ok(Self(ws_stream))
    }

    pub async fn subscribe(
        &mut self,
        subscription: LcuSubscriptionType,
    ) -> Result<(), LcuWebsocketError> {
        self.0
            .send(Message::text(format!("[5, \"{subscription}\"]")))
            .await
            .map_err(|e| match e {
                tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
                    LcuWebsocketError::Disconnected(e.to_string())
                }
                _ => LcuWebsocketError::SendError,
            })
    }

    pub async fn unsubscribe(
        &mut self,
        subscription: LcuSubscriptionType,
    ) -> Result<(), LcuWebsocketError> {
        self.0
            .send(Message::text(format!("[6, \"{subscription}\"]")))
            .await
            .map_err(|e| match e {
                tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
                    LcuWebsocketError::Disconnected(e.to_string())
                }
                _ => LcuWebsocketError::SendError,
            })
    }
}

impl Stream for WSClient {
    type Item = LcuEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            return match self.0.poll_next_unpin(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Some(Ok(Message::Text(text)))) => {
                    let Ok(de_event) = serde_json::from_str::<DeEvent>(&text) else { continue };
                    Poll::Ready(Some(LcuEvent::from(de_event)))
                }
                Poll::Ready(Some(Ok(Message::Close(_))) | Some(Err(_)) | None) => Poll::Ready(None),
                _ => continue,
            };
        }
    }
}
