# Runbook — operating environment

**Facts measured on the development machine on 2026-09-19, not assumptions.** Re-measure
before trusting any of this; every figure here is machine-specific and several are the
kind that change without anyone noticing.

## Docker address pool — the most imminent failure, and it is not disk

Docker's built-in default pool is `172.17.0.0/12` carved into /16s, giving fifteen usable
networks (`172.17`–`172.31`). Measured:

```
$ docker network ls -q | wc -l          → 16 networks
$ …inspect each, collect subnets        → 172.17 … 172.30, contiguous
$ docker info --format '{{json .DefaultAddressPools}}'  → null (built-in defaults)
```

**Fourteen of fifteen are already consumed** by unrelated projects on this machine
(`erp_default`, `news_default`, `rx_default`, several `fintant-*`, and others). One /16
remains.

### Measured, versus predicted — kept apart on purpose

**Measured:** fourteen of fifteen default /16s are consumed; this machine's LAN address is
`192.168.0.101` with gateway `192.168.0.1`, i.e. `192.168.0.0/24`.

**Documented behaviour:** when the `172.17.0.0/12` pool is exhausted Docker's next default
pool is `192.168.0.0/16`, allocated in /20 chunks — the first of which, `192.168.0.0/20`,
overlaps this machine's LAN.

**Not established:** that Docker *would* actually hand out the overlapping subnet. Its IPAM
performs some host-route conflict avoidance, and whether that is sufficient here has not
been tested — deliberately, because testing it means provoking the failure on a machine
someone is using. So this is a **potential conflict**, not a predicted outage.

The distinction matters for how much weight the mitigation carries: the harness allocates
from an explicitly configured block precisely so that none of this has to be predicted
correctly. The overlap is a reason not to rely on the default allocator, not a forecast.

**Consequence for the harness:** a single leaked network per attempt exhausts the
remaining /16 quickly, after which behaviour depends on a fallback path nobody here has
tested.
Networks are therefore allocated from an explicitly reserved block with a collision check
at allocation time, ownership tracked, and cleanup verified.

**On the reserved block.** `10.201.x.0/24` was proposed. Measured: no `10.x` routes exist
on the host (`netstat -rn -f inet`), and the only non-loopback address is the 192.168 LAN
one. Four `utun*` interfaces exist (VPN/tunnel), currently carrying no `10.x` routes — but
a VPN that comes up later could. **So the block is a configurable default, not a
reservation**, and the allocator checks for collision at the moment it allocates rather
than trusting this measurement to still hold.

The harness never modifies or removes a network it did not create.

## Disk

```
$ df -h ~          → 19Gi free of 460Gi (96% full)
```

Docker Desktop's `Docker.raw` grows to its high-water mark and does not shrink when space
is freed inside the VM. Reclaiming inside the VM (`docker builder prune`, `image prune`)
frees space for containers but does not necessarily return it to macOS.

Before a long run: prune, then **verify `Docker.raw` actually shrank** rather than assuming
it did. If it did not, the only reliable reclaim is Docker Desktop's own purge — a manual
operator step, deliberately not something the harness does to a shared machine.

## Toolchain

```
$ rustc --version  → 1.98.1 (pinned in rust-toolchain.toml)
```

The local `stable` was 1.93.1 (seven months behind) when this project started; the pin is
what makes that irrelevant. `cranelift` 0.135 declares MSRV 1.95.0, so the pin is
load-bearing for Stage 7 even though nothing depends on Cranelift yet.
