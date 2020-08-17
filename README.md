# udp-over-tcp

UDP traffic in a TCP stream.

## Installing

See `tcp2udp.service` for server side systemd service definition.

## Example

Make the server listen for TCP connections that it can then forward to a local UDP service.
This will listen on `0.0.0.0:5001/TCP` and forward anything that
comes in to `185.213.154.69:51820/UDP`:
```bash
user@server $ RUST_LOG=debug tcp2udp 0.0.0.0:5001 185.213.154.69:51820
```

Connect the client to the server above (assuming it lives on `10.0.0.1`) and have it listen
for incoming UDP on `127.0.0.1:51820/UDP`:
```bash
user@client $ RUST_LOG=debug udp2tcp 127.0.0.1:51820 10.0.0.1:5001
```

Now you can connect WireGuard to 127.0.0.1:51820 and have it be sent to 185.213.154.69:51820