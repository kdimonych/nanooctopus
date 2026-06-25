# hello_world.rs example

The simplest possible HTTP server built with **nanooctopus_server**.
It binds to `127.0.0.1:8080` and replies to every request with:

```http
HTTP/1.1 200 OK
Content-Type: text/plain

"Generic: Hello world!
```

Also, this example demonstrate handling several paths.

______________________________________________________________________

## What this example demonstrates

| Concept                                      | Where            |
| -------------------------------------------- | ---------------- |
| Implementing `Handler`                       | `RootHandler`    |
| Handle multiple paths                        | `map_handler!`   |
| Handling `/favicon.ico` request              | `FaviconHandler` |
| Running the server with `DefaultServer::run` | `main`           |

______________________________________________________________________

## Key components

### `RootHandler`

Implements the `Handler` trait.\
`handle` is called once per incoming HTTP request.\
The response is **streamed directly** to the socket in three stages using the `Connection`.

______________________________________________________________________

## Building and running

The example is a part of workspace and can be build and run with shortcut:

```sh
cargo run_hello_world_example
```

### Build only

```sh
cargo build_hello_world_example
```

The compiled binary is placed at `target/debug/examples/hello_world`.

### Build in release mode

```sh
cargo build_hello_world_example --release
```

Binary: `target/release/examples/hello_world`.

### Run (debug build)

```sh
cargo run_hello_world_example
```

### Run (release build)

```sh
cargo run_hello_world_example --release
```

Then in another terminal:

```sh
curl http://127.0.0.1:8080/
# Generic: Hello world!
```

Or with verbose HTTP output:

```sh
curl -v http://127.0.0.1:8080/
```

### Check (no binary produced, fastest feedback)

```sh
cargo check
```

______________________________________________________________________

## Dependencies

| Crate                                         | Purpose                           |
| --------------------------------------------- | --------------------------------- |
| `nanooctopus_server` (features: `std`, `log`) | HTTP server library               |
| `tokio` (features: `full`)                    | Async runtime                     |
| `log` + `env_logger`                          | Structured logging via `RUST_LOG` |

Set `RUST_LOG` to control log verbosity. The variable is read at startup by
`env_logger` — it does **not** require a recompile.

| Value                       | What you see                                       |
| --------------------------- | -------------------------------------------------- |
| `RUST_LOG=error`            | Only errors                                        |
| `RUST_LOG=info` *(default)* | Incoming requests and server lifecycle events      |
| `RUST_LOG=debug`            | Internal parser state, socket events, and timeouts |
| `RUST_LOG=trace`            | All of the above plus low-level byte I/O           |

### macOS / Linux

```sh
RUST_LOG=debug cargo run_hello_world_example
```

### Windows (PowerShell)

```powershell
$env:RUST_LOG="debug"; cargo run_hello_world_example
```

### Windows (cmd)

```cmd
set RUST_LOG=debug && cargo run_hello_world_example
```

To restrict verbose output to nanooctopus only (leaving other crates quiet):

```sh
RUST_LOG=nanooctopus=debug cargo run_hello_world_example
```
