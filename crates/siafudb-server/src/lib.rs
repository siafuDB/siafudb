// siafudb-server — Optional standalone server for development and testing.
//
// Most SiafuDB instances are embedded — they run inside an app, not as
// a separate process. But for development, testing, and some cloud
// deployments, a standalone server is useful.
//
// The server exposes the same SiafuDB capabilities over HTTP/gRPC,
// including the sync protocol endpoints that adapters connect to.
//
// TODO: Phase 5 implementation
