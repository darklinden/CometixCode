//! MCP WebSocket transport.
//! Maps to: CC `utils/mcpWebSocketTransport.ts`.
//!
//! CC wraps an already-created WebSocket in `WebSocketTransport`. Rust builds
//! the WebSocket request here because `tokio-tungstenite` combines the
//! JavaScript `new WebSocket(url, 'mcp', headers)` step with the connection.

#[cfg(feature = "mcp_runtime")]
mod runtime {
    use futures::stream::BoxStream;
    use futures::{SinkExt, StreamExt};
    use http::{HeaderName, HeaderValue};
    use rmcp::RoleClient;
    use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
    use rmcp::transport::Transport as RmcpTransport;
    use std::collections::BTreeMap;
    use std::fmt;
    use std::future::Future;
    use std::str::FromStr;
    use std::sync::Arc;
    use tokio::net::TcpStream;
    use tokio::sync::Mutex;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::handshake::client::Request;
    use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
    use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

    type WsMcpStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

    /// Maps to: CC `utils/mcpWebSocketTransport.ts#WebSocketTransport`.
    pub(crate) struct WebSocketTransport {
        sink: Arc<Mutex<futures::stream::SplitSink<WsMcpStream, WsMessage>>>,
        incoming: BoxStream<'static, RxJsonRpcMessage<RoleClient>>,
    }

    #[derive(Debug)]
    pub(crate) struct WebSocketTransportError(anyhow::Error);

    impl fmt::Display for WebSocketTransportError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for WebSocketTransportError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.0.source()
        }
    }

    impl RmcpTransport<RoleClient> for WebSocketTransport {
        type Error = WebSocketTransportError;

        fn send(
            &mut self,
            item: TxJsonRpcMessage<RoleClient>,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
            let sink = self.sink.clone();
            async move {
                // Maps to: CC `utils/mcpWebSocketTransport.ts#send`.
                let payload = serde_json::to_string(&item)
                    .map_err(|error| WebSocketTransportError(anyhow::Error::new(error)))?;
                sink.lock()
                    .await
                    .send(WsMessage::Text(payload.into()))
                    .await
                    .map_err(|error| WebSocketTransportError(anyhow::Error::new(error)))
            }
        }

        fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleClient>>> + Send {
            self.incoming.next()
        }

        async fn close(&mut self) -> Result<(), Self::Error> {
            // Maps to: CC `utils/mcpWebSocketTransport.ts#close`.
            let _ = self.sink.lock().await.send(WsMessage::Close(None)).await;
            Ok(())
        }
    }

    fn parse_websocket_message(
        server_name: &str,
        payload: &str,
    ) -> Option<RxJsonRpcMessage<RoleClient>> {
        // Maps to: CC `utils/mcpWebSocketTransport.ts#onNodeMessage` /
        // `onBunMessage` JSONRPCMessageSchema parse.
        match serde_json::from_str::<RxJsonRpcMessage<RoleClient>>(payload) {
            Ok(message) => Some(message),
            Err(error) => {
                tracing::warn!(server = server_name, error = %error, "failed to parse MCP WebSocket message");
                None
            }
        }
    }

    fn build_websocket_request(
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> anyhow::Result<Request> {
        // Maps to: CC `new WebSocket(serverRef.url, 'mcp', { headers })`.
        let mut request = url.into_client_request()?;
        request
            .headers_mut()
            .insert("Sec-WebSocket-Protocol", HeaderValue::from_static("mcp"));
        for (key, value) in headers {
            let name = HeaderName::from_str(&key)?;
            let value = HeaderValue::from_str(&value)?;
            request.headers_mut().insert(name, value);
        }
        Ok(request)
    }

    /// Maps to: CC `utils/mcpWebSocketTransport.ts#WebSocketTransport.constructor`
    /// plus the `client.ts` WebSocket construction site.
    pub(crate) async fn connect_mcp_websocket_transport(
        server_name: &str,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> anyhow::Result<WebSocketTransport> {
        let request = build_websocket_request(url, headers)?;
        let (stream, _response) = connect_async(request).await?;
        let (sink, source) = stream.split();
        let name = server_name.to_string();
        let incoming = source.filter_map(move |message| {
            let name = name.clone();
            async move {
                let message = match message {
                    Ok(message) => message,
                    Err(error) => {
                        tracing::warn!(server = %name, error = %error, "WebSocket MCP stream error");
                        return None;
                    }
                };
                match message {
                    WsMessage::Text(payload) => parse_websocket_message(&name, &payload),
                    WsMessage::Binary(payload) => std::str::from_utf8(&payload)
                        .ok()
                        .and_then(|payload| parse_websocket_message(&name, payload)),
                    WsMessage::Close(_) => None,
                    WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Frame(_) => None,
                }
            }
        });

        Ok(WebSocketTransport {
            sink: Arc::new(Mutex::new(sink)),
            incoming: incoming.boxed(),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn websocket_request_sets_mcp_subprotocol_and_headers_like_official_transport() {
            let request = build_websocket_request(
                "ws://127.0.0.1:9999/mcp",
                BTreeMap::from([("Authorization".to_string(), "Bearer token".to_string())]),
            )
            .unwrap();
            assert_eq!(
                request.headers().get("Sec-WebSocket-Protocol"),
                Some(&HeaderValue::from_static("mcp"))
            );
            assert_eq!(
                request.headers().get("Authorization"),
                Some(&HeaderValue::from_static("Bearer token"))
            );
        }
    }
}

#[cfg(feature = "mcp_runtime")]
pub(crate) use runtime::connect_mcp_websocket_transport;
