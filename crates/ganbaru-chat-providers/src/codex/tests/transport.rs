use super::*;

#[test]
fn transport_correlates_out_of_order_responses_and_routes_messages() {
    crate::test_block_on(async {
        let (client_reader, mut server_writer) = tokio::io::duplex(16 * 1024);
        let (server_reader, client_writer) = tokio::io::duplex(16 * 1024);
        let mut connection = CodexRpcConnection::from_test_io(client_reader, client_writer);
        let client = connection.client();
        let mut inbound = connection.take_inbound().unwrap();
        let mut requests = BufReader::new(server_reader);
        let first_client = client.clone();
        let first_context = context("first");
        let first = tokio::spawn(async move {
            first_client
                .request("test/first", json!({ "value": 1 }), &first_context)
                .await
        });
        let second_client = client.clone();
        let second_context = context("second");
        let second = tokio::spawn(async move {
            second_client
                .request("test/second", json!({ "value": 2 }), &second_context)
                .await
        });
        let mut request_lines = Vec::new();
        for _ in 0..2 {
            let mut line = String::new();
            requests.read_line(&mut line).await.unwrap();
            request_lines.push(serde_json::from_str::<Value>(&line).unwrap());
        }
        let first_request = request_lines
            .iter()
            .find(|request| request["method"] == "test/first")
            .unwrap();
        let second_request = request_lines
            .iter()
            .find(|request| request["method"] == "test/second")
            .unwrap();
        server_writer
            .write_all(
                format!(
                    "{{\"id\":{},\"result\":{{\"name\":\"second\"}}}}\n",
                    second_request["id"]
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        server_writer
            .write_all(
                format!(
                    "{{\"id\":{},\"result\":{{\"name\":\"first\"}}}}\n",
                    first_request["id"]
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        server_writer
            .write_all(b"{\"method\":\"notice/test\",\"params\":{\"ok\":true}}\n")
            .await
            .unwrap();
        server_writer
            .write_all(b"{\"id\":\"server-1\",\"method\":\"request/test\",\"params\":{}}\n")
            .await
            .unwrap();

        assert_eq!(first.await.unwrap().unwrap()["name"], "first");
        assert_eq!(second.await.unwrap().unwrap()["name"], "second");
        assert_eq!(
            inbound.recv().await.unwrap(),
            CodexInboundMessage::Notification {
                method: "notice/test".to_string(),
                params: json!({ "ok": true }),
            }
        );
        assert_eq!(
            inbound.recv().await.unwrap(),
            CodexInboundMessage::Request {
                id: json!("server-1"),
                method: "request/test".to_string(),
                params: json!({}),
            }
        );
        connection
            .stop(Duration::from_millis(10), Duration::from_millis(10))
            .await
            .unwrap();
    });
}

#[test]
fn transport_reports_malformed_input_without_echoing_raw_content() {
    crate::test_block_on(async {
        let (client_reader, mut server_writer) = tokio::io::duplex(1024);
        let (_server_reader, client_writer) = tokio::io::duplex(1024);
        let mut connection = CodexRpcConnection::from_test_io(client_reader, client_writer);
        let mut inbound = connection.take_inbound().unwrap();
        server_writer
            .write_all(b"not-json-with-ganbaru-secret\n")
            .await
            .unwrap();

        let CodexInboundMessage::Malformed {
            reason,
            byte_length,
        } = inbound.recv().await.unwrap()
        else {
            panic!("expected malformed input");
        };
        assert_eq!(reason, "message is not valid JSON");
        assert_eq!(byte_length, 28);
        assert!(!reason.contains("ganbaru-secret"));
    });
}

#[test]
fn transport_honors_cancellation_while_waiting_for_response() {
    crate::test_block_on(async {
        let (client_reader, _server_writer) = tokio::io::duplex(1024);
        let (_server_reader, client_writer) = tokio::io::duplex(1024);
        let connection = CodexRpcConnection::from_test_io(client_reader, client_writer);
        let client = connection.client();
        let cancellation = DriverCancellation::default();
        let request_context = DriverOperationContext {
            operation_id: "cancelled".to_string(),
            deadline: Instant::now() + Duration::from_secs(2),
            cancellation: cancellation.clone(),
        };
        let request = tokio::spawn(async move {
            client
                .request("test/cancel", json!({}), &request_context)
                .await
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancellation.cancel();
        assert_eq!(request.await.unwrap(), Err(CodexRpcFailure::Cancelled));
    });
}
