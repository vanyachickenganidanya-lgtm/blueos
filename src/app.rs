use crate::{
    framebuffer::Ui,
    lua,
    net::{Event, NetworkStack, Nic},
};

pub fn banner(ui: &mut Ui, architecture: &str, nic_found: bool) {
    ui.write("BlueOS 0.1 - Rust + assembly kernel\n");
    ui.write("Architecture: ");
    ui.write(architecture);
    ui.write("\nGraphics framebuffer: ready\n");
    ui.write("Lua compiler/VM: ready\n");
    if nic_found {
        ui.write("Ethernet driver: ready (QEMU user network)\n");
    } else {
        ui.write("Ethernet driver: device not found\n");
    }
    ui.write("Type HELP for commands.\n\n> ");
}

pub fn execute<N: Nic>(
    command: &str,
    ui: &mut Ui,
    network: &mut Option<(N, NetworkStack)>,
    architecture: &str,
) {
    let command = command.trim();
    if command.eq_ignore_ascii_case("help") {
        ui.write("HELP  INFO  CLEAR  LUA  NET  DNS\n");
    } else if command.eq_ignore_ascii_case("info") {
        ui.write("BlueOS kernel on ");
        ui.write(architecture);
        ui.write(". No std, no allocator, identity-mapped hardware.\n");
    } else if command.eq_ignore_ascii_case("clear") {
        ui.clear();
        return;
    } else if command.eq_ignore_ascii_case("lua") {
        run_lua_demo(ui);
    } else if command.eq_ignore_ascii_case("net") {
        match network {
            Some((nic, stack)) => {
                ui.write("NIC online, IP ");
                ui.write_ipv4(stack.local_ip());
                ui.write(", gateway ARP: ");
                ui.write(if stack.gateway_ready() { "ready" } else { "waiting" });
                ui.write(", RX=");
                ui.write_number(stack.received() as i64);
                ui.write(" TX=");
                ui.write_number(stack.sent() as i64);
                ui.write("\n");
                stack.start(nic);
            }
            None => ui.write("No supported network adapter was found.\n"),
        }
    } else if command.eq_ignore_ascii_case("dns") {
        match network {
            Some((nic, stack)) => {
                ui.write("Resolving example.com through 10.0.2.3...\n");
                stack.request_dns(nic);
            }
            None => ui.write("Network is unavailable.\n"),
        }
    } else if command.is_empty() {
        // Just print another prompt.
    } else {
        ui.write("Unknown command. Type HELP.\n");
    }
    ui.write("> ");
}

pub fn run_lua_demo(ui: &mut Ui) {
    ui.write("Compiling embedded Lua source to bytecode...\n");
    match lua::compile(lua::DEMO) {
        Ok(program) => {
            let result = program.run(|value| match value {
                lua::Output::Integer(number) => {
                    ui.write("[lua] ");
                    ui.write_number(number);
                    ui.write("\n");
                }
                lua::Output::String(text) => {
                    ui.write("[lua] ");
                    ui.write(text);
                    ui.write("\n");
                }
            });
            if let Err(error) = result {
                ui.write("Lua VM error: ");
                ui.write(error);
                ui.write("\n");
            }
        }
        Err(error) => {
            ui.write("Lua compile error: ");
            ui.write(error);
            ui.write("\n");
        }
    }
}

pub fn show_network_event(ui: &mut Ui, event: Event) {
    match event {
        Event::GatewayResolved => ui.write("\n[net] gateway MAC resolved; DNS query sent\n> "),
        Event::IcmpEchoRequest(address) => {
            ui.write("\n[net] answered ICMP echo from ");
            ui.write_ipv4(address);
            ui.write("\n> ");
        }
        Event::IcmpEchoReply(address) => {
            ui.write("\n[net] ICMP echo reply from ");
            ui.write_ipv4(address);
            ui.write("\n> ");
        }
        Event::DnsAnswer(address) => {
            ui.write("\n[net] example.com A = ");
            ui.write_ipv4(address);
            ui.write("\n> ");
        }
    }
}
