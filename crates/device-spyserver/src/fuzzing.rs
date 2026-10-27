use crate::proto::{ClientSync, DeviceInfo, HEADER_LEN, MAX_BODY, MessageHeader};

pub fn read_message(bytes: &[u8]) {
    if let Some(head) = bytes.first_chunk::<HEADER_LEN>()
        && let Ok(header) = MessageHeader::parse(head)
    {
        assert!(
            header.body_size <= MAX_BODY,
            "SpyServer accepted a body of {} bytes",
            header.body_size
        );
    }
    let _ = DeviceInfo::parse(bytes);
    let _ = ClientSync::parse(bytes);
}
