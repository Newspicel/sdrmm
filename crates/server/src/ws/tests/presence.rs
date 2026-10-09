use sdrmm_wire::{DraggedNode, Pointer, Position};

use super::*;

async fn present(ws: &mut WsClient, author: &str, name: &str) -> (u32, Vec<sdrmm_wire::Peer>) {
    send(
        ws,
        &ClientCommand::Present {
            author: author.to_owned(),
            name: name.to_owned(),
        },
    )
    .await;
    peers(ws).await
}

async fn peers(ws: &mut WsClient) -> (u32, Vec<sdrmm_wire::Peer>) {
    loop {
        if let ServerEvent::Peers { you, peers } = next_event(ws).await {
            return (you, peers);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn peers_see_each_other_join_point_and_leave() {
    let (addr, _state) = serve_ws(test_engine()).await;
    let mut ann = dial(addr).await;
    let mut bob = dial(addr).await;

    let (ann_id, alone) = present(&mut ann, "ann-key", "Ann").await;
    assert_eq!(alone.len(), 1);
    assert_eq!(alone[0].name, "Ann");
    let (bob_id, both) = present(&mut bob, "bob-key", "Bob").await;
    assert_eq!(both.len(), 2);
    let (_, seen) = peers(&mut ann).await;
    assert!(
        seen.iter()
            .any(|peer| peer.id == bob_id && peer.name == "Bob")
    );

    let pointer = Pointer {
        at: Some(Position { x: 10.0, y: 20.0 }),
        selected: vec!["scope".to_owned()],
        dragging: vec![DraggedNode {
            node: "scope".to_owned(),
            position: Position { x: 1.0, y: 2.0 },
        }],
    };
    send(&mut bob, &ClientCommand::Point(pointer.clone())).await;
    loop {
        if let ServerEvent::PeerPointer { peer, pointer: got } = next_event(&mut ann).await {
            assert_eq!(peer, bob_id);
            assert_eq!(got, pointer);
            break;
        }
    }

    drop(bob);
    let (_, left) = peers(&mut ann).await;
    assert_eq!(
        left.iter().map(|peer| peer.id).collect::<Vec<_>>(),
        vec![ann_id]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pointing_before_presenting_is_refused() {
    let mut ws = connect(test_engine()).await;
    send(&mut ws, &ClientCommand::Point(Pointer::default())).await;
    loop {
        if let ServerEvent::Error { message } = next_event(&mut ws).await {
            assert!(message.contains("Present"), "{message}");
            break;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_author_key_is_refused() {
    let mut ws = connect(test_engine()).await;
    send(
        &mut ws,
        &ClientCommand::Present {
            author: "not a key".to_owned(),
            name: String::new(),
        },
    )
    .await;
    loop {
        if let ServerEvent::Error { message } = next_event(&mut ws).await {
            assert!(message.contains("author"), "{message}");
            break;
        }
    }
}
