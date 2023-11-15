use crate::protowire;
use crate::{from, try_from};
use kaspa_rpc_core::RpcError;

// ----------------------------------------------------------------------------
// rpc_core to protowire
// ----------------------------------------------------------------------------

from!(item: &kaspa_rpc_core::ProcessMetrics, protowire::ProcessMetrics, {
    Self {
        resident_set_size: item.resident_set_size,
        virtual_memory_size: item.virtual_memory_size,
        core_num: item.core_num,
        cpu_usage: item.cpu_usage,
        fd_num: item.fd_num,
        disk_io_read_bytes: item.disk_io_read_bytes,
        disk_io_write_bytes: item.disk_io_write_bytes,
        disk_io_read_per_sec: item.disk_io_read_per_sec,
        disk_io_write_per_sec: item.disk_io_write_per_sec,
    }
});

from!(item: &kaspa_rpc_core::ConnectionMetrics, protowire::ConnectionMetrics, {
    Self {
        borsh_live_connections: item.borsh_live_connections,
        borsh_connection_attempts: item.borsh_connection_attempts,
        borsh_handshake_failures: item.borsh_handshake_failures,
        json_live_connections: item.json_live_connections,
        json_connection_attempts: item.json_connection_attempts,
        json_handshake_failures: item.json_handshake_failures,
        active_peers: item.active_peers,
    }
});

from!(item: &kaspa_rpc_core::ConsensusMetrics, protowire::ConsensusMetrics, {
    Self {
        blocks_submitted: item.blocks_submitted,
        header_counts: item.header_counts,
        dep_counts: item.dep_counts,
        body_counts: item.body_counts,
        txs_counts: item.txs_counts,
        chain_block_counts: item.chain_block_counts,
        mass_counts: item.mass_counts,

        block_count: item.block_count,
        header_count: item.header_count,
        tip_hashes_count: item.tip_hashes_count,
        difficulty: item.difficulty,
        past_median_time: item.past_median_time,
        virtual_parent_hashes_count: item.virtual_parent_hashes_count,
        virtual_daa_score: item.virtual_daa_score,
    }
});

// ----------------------------------------------------------------------------
// protowire to rpc_core
// ----------------------------------------------------------------------------

try_from!(item: &protowire::ProcessMetrics, kaspa_rpc_core::ProcessMetrics, {
    Self {
        resident_set_size: item.resident_set_size,
        virtual_memory_size: item.virtual_memory_size,
        core_num: item.core_num,
        cpu_usage: item.cpu_usage,
        fd_num: item.fd_num,
        disk_io_read_bytes: item.disk_io_read_bytes,
        disk_io_write_bytes: item.disk_io_write_bytes,
        disk_io_read_per_sec: item.disk_io_read_per_sec,
        disk_io_write_per_sec: item.disk_io_write_per_sec,
    }
});

try_from!(item: &protowire::ConnectionMetrics, kaspa_rpc_core::ConnectionMetrics, {
    Self {
        borsh_live_connections: item.borsh_live_connections,
        borsh_connection_attempts: item.borsh_connection_attempts,
        borsh_handshake_failures: item.borsh_handshake_failures,
        json_live_connections: item.json_live_connections,
        json_connection_attempts: item.json_connection_attempts,
        json_handshake_failures: item.json_handshake_failures,
        active_peers: item.active_peers,
    }
});

try_from!(item: &protowire::ConsensusMetrics, kaspa_rpc_core::ConsensusMetrics, {
    Self {
        blocks_submitted: item.blocks_submitted,
        header_counts: item.header_counts,
        dep_counts: item.dep_counts,
        body_counts: item.body_counts,
        txs_counts: item.txs_counts,
        chain_block_counts: item.chain_block_counts,
        mass_counts: item.mass_counts,

        block_count: item.block_count,
        header_count: item.header_count,
        tip_hashes_count: item.tip_hashes_count,
        difficulty: item.difficulty,
        past_median_time: item.past_median_time,
        virtual_parent_hashes_count: item.virtual_parent_hashes_count,
        virtual_daa_score: item.virtual_daa_score,
    }
});
