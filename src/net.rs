//! Minimal allocation-free Ethernet/ARP/IPv4/ICMP/UDP/DNS stack.
//! The default addresses match QEMU's user-mode network (SLIRP).

pub trait Nic {
    fn mac_address(&self) -> [u8; 6];
    fn receive(&mut self, packet: &mut [u8]) -> Option<usize>;
    fn transmit(&mut self, packet: &[u8]) -> bool;
}

const LOCAL_IP: [u8; 4] = [10, 0, 2, 15];
const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
const DNS_IP: [u8; 4] = [10, 0, 2, 3];
const DNS_PORT: u16 = 49_152;

#[derive(Clone, Copy)]
pub enum Event {
    GatewayResolved,
    IcmpEchoRequest([u8; 4]),
    IcmpEchoReply([u8; 4]),
    DnsAnswer([u8; 4]),
}

pub struct NetworkStack {
    local_mac: [u8; 6],
    gateway_mac: Option<[u8; 6]>,
    received: u64,
    sent: u64,
    dns_sent: bool,
}

impl NetworkStack {
    pub fn new(local_mac: [u8; 6]) -> Self {
        Self {
            local_mac,
            gateway_mac: None,
            received: 0,
            sent: 0,
            dns_sent: false,
        }
    }

    pub fn local_ip(&self) -> [u8; 4] {
        LOCAL_IP
    }

    pub fn received(&self) -> u64 {
        self.received
    }

    pub fn sent(&self) -> u64 {
        self.sent
    }

    pub fn gateway_ready(&self) -> bool {
        self.gateway_mac.is_some()
    }

    pub fn start<N: Nic>(&mut self, nic: &mut N) {
        self.send_arp_request(nic);
    }

    pub fn request_dns<N: Nic>(&mut self, nic: &mut N) {
        if self.gateway_mac.is_none() {
            self.send_arp_request(nic);
        } else {
            self.send_dns_query(nic);
        }
    }

    pub fn poll<N: Nic>(&mut self, nic: &mut N) -> Option<Event> {
        let mut packet = [0u8; 2048];
        let length = nic.receive(&mut packet)?;
        if length < 14 {
            return None;
        }
        self.received += 1;
        match u16::from_be_bytes([packet[12], packet[13]]) {
            0x0806 => self.handle_arp(nic, &packet[..length]),
            0x0800 => self.handle_ipv4(nic, &packet[..length]),
            _ => None,
        }
    }

    fn handle_arp<N: Nic>(&mut self, nic: &mut N, packet: &[u8]) -> Option<Event> {
        if packet.len() < 42 || packet[14..20] != [0, 1, 8, 0, 6, 4] {
            return None;
        }
        let operation = u16::from_be_bytes([packet[20], packet[21]]);
        let sender_mac: [u8; 6] = packet[22..28].try_into().ok()?;
        let sender_ip: [u8; 4] = packet[28..32].try_into().ok()?;
        let target_ip: [u8; 4] = packet[38..42].try_into().ok()?;

        if operation == 1 && target_ip == LOCAL_IP {
            self.send_arp_reply(nic, sender_mac, sender_ip);
        }
        if operation == 2 && sender_ip == GATEWAY_IP {
            self.gateway_mac = Some(sender_mac);
            if !self.dns_sent {
                self.send_dns_query(nic);
            }
            return Some(Event::GatewayResolved);
        }
        None
    }

    fn handle_ipv4<N: Nic>(&mut self, nic: &mut N, packet: &[u8]) -> Option<Event> {
        if packet.len() < 34 || packet[14] >> 4 != 4 {
            return None;
        }
        let header_length = ((packet[14] & 0x0f) as usize) * 4;
        if header_length < 20 || 14 + header_length > packet.len() {
            return None;
        }
        let source: [u8; 4] = packet[26..30].try_into().ok()?;
        let destination: [u8; 4] = packet[30..34].try_into().ok()?;
        if destination != LOCAL_IP {
            return None;
        }
        let payload_offset = 14 + header_length;
        match packet[23] {
            1 if packet.len() >= payload_offset + 8 => {
                let kind = packet[payload_offset];
                if kind == 8 {
                    self.send_icmp_reply(nic, packet, payload_offset, source);
                    Some(Event::IcmpEchoRequest(source))
                } else if kind == 0 {
                    Some(Event::IcmpEchoReply(source))
                } else {
                    None
                }
            }
            17 => self.parse_dns(packet, payload_offset),
            _ => None,
        }
    }

    fn parse_dns(&self, packet: &[u8], udp: usize) -> Option<Event> {
        if packet.len() < udp + 20 {
            return None;
        }
        let source_port = u16::from_be_bytes([packet[udp], packet[udp + 1]]);
        let destination_port = u16::from_be_bytes([packet[udp + 2], packet[udp + 3]]);
        if source_port != 53 || destination_port != DNS_PORT {
            return None;
        }
        let dns = udp + 8;
        if packet.len() < dns + 12 || packet[dns..dns + 2] != [0xb1, 0x05] {
            return None;
        }
        let questions = u16::from_be_bytes([packet[dns + 4], packet[dns + 5]]) as usize;
        let answers = u16::from_be_bytes([packet[dns + 6], packet[dns + 7]]) as usize;
        let mut cursor = dns + 12;
        for _ in 0..questions {
            cursor = skip_dns_name(packet, cursor)?;
            cursor = cursor.checked_add(4)?;
        }
        for _ in 0..answers {
            cursor = skip_dns_name(packet, cursor)?;
            if cursor + 10 > packet.len() {
                return None;
            }
            let kind = u16::from_be_bytes([packet[cursor], packet[cursor + 1]]);
            let class = u16::from_be_bytes([packet[cursor + 2], packet[cursor + 3]]);
            let data_length = u16::from_be_bytes([packet[cursor + 8], packet[cursor + 9]]) as usize;
            cursor += 10;
            if cursor + data_length > packet.len() {
                return None;
            }
            if kind == 1 && class == 1 && data_length == 4 {
                return Some(Event::DnsAnswer(packet[cursor..cursor + 4].try_into().ok()?));
            }
            cursor += data_length;
        }
        None
    }

    fn send_arp_request<N: Nic>(&mut self, nic: &mut N) {
        let mut frame = [0u8; 60];
        frame[..6].fill(0xff);
        frame[6..12].copy_from_slice(&self.local_mac);
        frame[12..14].copy_from_slice(&0x0806u16.to_be_bytes());
        frame[14..22].copy_from_slice(&[0, 1, 8, 0, 6, 4, 0, 1]);
        frame[22..28].copy_from_slice(&self.local_mac);
        frame[28..32].copy_from_slice(&LOCAL_IP);
        frame[32..38].fill(0);
        frame[38..42].copy_from_slice(&GATEWAY_IP);
        if nic.transmit(&frame) {
            self.sent += 1;
        }
    }

    fn send_arp_reply<N: Nic>(&mut self, nic: &mut N, target_mac: [u8; 6], target_ip: [u8; 4]) {
        let mut frame = [0u8; 60];
        frame[..6].copy_from_slice(&target_mac);
        frame[6..12].copy_from_slice(&self.local_mac);
        frame[12..14].copy_from_slice(&0x0806u16.to_be_bytes());
        frame[14..22].copy_from_slice(&[0, 1, 8, 0, 6, 4, 0, 2]);
        frame[22..28].copy_from_slice(&self.local_mac);
        frame[28..32].copy_from_slice(&LOCAL_IP);
        frame[32..38].copy_from_slice(&target_mac);
        frame[38..42].copy_from_slice(&target_ip);
        if nic.transmit(&frame) {
            self.sent += 1;
        }
    }

    fn send_icmp_reply<N: Nic>(
        &mut self,
        nic: &mut N,
        request: &[u8],
        payload_offset: usize,
        destination_ip: [u8; 4],
    ) {
        let ip_length = u16::from_be_bytes([request[16], request[17]]) as usize;
        let frame_length = (14 + ip_length).min(request.len()).min(1514);
        let mut reply = [0u8; 1514];
        reply[..frame_length].copy_from_slice(&request[..frame_length]);
        reply[..6].copy_from_slice(&request[6..12]);
        reply[6..12].copy_from_slice(&self.local_mac);
        reply[26..30].copy_from_slice(&LOCAL_IP);
        reply[30..34].copy_from_slice(&destination_ip);
        reply[24] = 0;
        reply[25] = 0;
        let ip_checksum = checksum(&reply[14..payload_offset]);
        reply[24..26].copy_from_slice(&ip_checksum.to_be_bytes());
        reply[payload_offset] = 0;
        reply[payload_offset + 2] = 0;
        reply[payload_offset + 3] = 0;
        let icmp_checksum = checksum(&reply[payload_offset..frame_length]);
        reply[payload_offset + 2..payload_offset + 4]
            .copy_from_slice(&icmp_checksum.to_be_bytes());
        if nic.transmit(&reply[..frame_length]) {
            self.sent += 1;
        }
    }

    fn send_dns_query<N: Nic>(&mut self, nic: &mut N) {
        let gateway = match self.gateway_mac {
            Some(mac) => mac,
            None => return,
        };
        const QUESTION: &[u8] = b"\x07example\x03com\x00\x00\x01\x00\x01";
        let dns_length = 12 + QUESTION.len();
        let udp_length = 8 + dns_length;
        let ip_length = 20 + udp_length;
        let frame_length = 14 + ip_length;
        let mut frame = [0u8; 128];
        frame[..6].copy_from_slice(&gateway);
        frame[6..12].copy_from_slice(&self.local_mac);
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        frame[14] = 0x45;
        frame[16..18].copy_from_slice(&(ip_length as u16).to_be_bytes());
        frame[18..20].copy_from_slice(&0xb105u16.to_be_bytes());
        frame[20..22].copy_from_slice(&0x4000u16.to_be_bytes());
        frame[22] = 64;
        frame[23] = 17;
        frame[26..30].copy_from_slice(&LOCAL_IP);
        frame[30..34].copy_from_slice(&DNS_IP);
        let ip_checksum = checksum(&frame[14..34]);
        frame[24..26].copy_from_slice(&ip_checksum.to_be_bytes());
        frame[34..36].copy_from_slice(&DNS_PORT.to_be_bytes());
        frame[36..38].copy_from_slice(&53u16.to_be_bytes());
        frame[38..40].copy_from_slice(&(udp_length as u16).to_be_bytes());
        // UDP checksum 0 is legal for IPv4.
        frame[42..54].copy_from_slice(&[
            0xb1, 0x05, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]);
        frame[54..54 + QUESTION.len()].copy_from_slice(QUESTION);
        if nic.transmit(&frame[..frame_length]) {
            self.sent += 1;
            self.dns_sent = true;
        }
    }
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut chunks = bytes.chunks_exact(2);
    for chunk in &mut chunks {
        sum = sum.wrapping_add(u16::from_be_bytes([chunk[0], chunk[1]]) as u32);
    }
    if let Some(last) = chunks.remainder().first() {
        sum = sum.wrapping_add((*last as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn skip_dns_name(packet: &[u8], mut cursor: usize) -> Option<usize> {
    loop {
        let length = *packet.get(cursor)?;
        cursor += 1;
        if length == 0 {
            return Some(cursor);
        }
        if length & 0xc0 == 0xc0 {
            packet.get(cursor)?;
            return Some(cursor + 1);
        }
        cursor = cursor.checked_add(length as usize)?;
        if cursor > packet.len() {
            return None;
        }
    }
}
