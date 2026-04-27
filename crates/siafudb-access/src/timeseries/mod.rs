// siafudb-access/src/timeseries/mod.rs
//
// TIME-SERIES ACCESS PATTERN
//
// Store and query time-stamped data points.
// Each data point is a graph node connected to its series by a NEXT edge,
// forming a temporal chain that can be traversed forward or backward.
//
// This is how SiafuDB handles: activity logs, health data, sensor readings,
// analytics events, engagement history — anything that's a sequence of
// timestamped observations.
//
// TODO: Phase 1 implementation
// - ts_append(series, timestamp, value) → creates timestamped node
// - ts_range(series, start, end) → traverses temporal chain
// - ts_latest(series, n) → last N data points
// - ts_aggregate(series, start, end, fn) → sum/avg/count/min/max
// - Columnar compression for storage efficiency (Phase 2)
