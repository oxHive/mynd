# Pairing devices into a Hive

Hive Mode syncs memories directly between your own devices over mTLS. There's
no central server, and no third party ever holds your data. Each device is a
peer; pairing two devices lets them gossip-sync with each other (and,
transitively, with anything else already in each other's roster).

Prerequisites on **both** devices:

- `[hive] enabled = true` in `~/.config/mynd/config.toml`
- `mynd up` (or the background service) running

## Dashboard pairing (has a display)

Open the dashboard's **Settings > Hive** tab. On the inviting device, generate
a pairing code; on the joining device, enter the code plus the inviter's
address and public key (shown alongside the code). This is the easiest path
when both devices have a browser.

## CLI pairing (headless, e.g. a Raspberry Pi)

The dashboard's pairing endpoints are intentionally reachable only from
`127.0.0.1` on the device itself. That's what stops anyone who can merely
reach your LAN dashboard port from minting themselves a pairing code. On a
headless box that means the dashboard flow needs a display it doesn't have.
`mynd hive pair`/`mynd hive join` wrap the same endpoints from the CLI
instead, which works over a plain SSH session since the request still
originates from the box itself.

It doesn't matter which device runs `pair` and which runs `join`. Both
commands are plain CLI/SSH commands with no display requirement, so a headless
Pi can play either role. Pick whichever device you're on first to run `pair`
(it issues the code); the other device runs `join` to redeem it.

**On the inviting device** (e.g. SSH into the Pi):

```sh
mynd hive pair
```

```text
Pairing code:  XK7QRT2P
Expires in:    287s

On the OTHER device, run:
  mynd hive join XK7QRT2P 192.168.1.42:3458 3a1f...e02c
```

**On the joining device**, run the printed command (or type it in by hand):

```sh
mynd hive join XK7QRT2P 192.168.1.42:3458 3a1f...e02c
```

```text
Joined the hive. Roster now has 2 device(s).
```

The pairing code expires after 5 minutes and is single-use; run `mynd hive
pair` again if it lapses before you redeem it.

### Why this solves the "devices never overlap online" gap

Hive Mode's sync is peer-to-peer, but nothing requires every pair of devices
to be online at the same time. Only *paired* devices need to have gossiped at
some point. Pairing your laptop and phone with an always-on box (a Raspberry
Pi, a home server) turns that box into a reachable hub: your laptop and phone
each sync with it whenever *they're* online, instead of needing to overlap
with each other directly.

### Troubleshooting

- **"Hive Mode is not enabled"**: set `[hive] enabled = true` in
  `~/.config/mynd/config.toml` on that device and restart `mynd up`.
- **"mynd up is not running on this device"**: start it first. Pairing state
  lives in the running server process, not on disk.
- **"could not issue a pairing code" / "could not join the hive"**: the
  server's own rejection reason is included (expired/invalid code, peer
  unreachable, or the joining device was previously revoked from this hive).
- **`mynd up` won't stay running on the headless box, or the device's
  identity keeps changing after every reboot**: see [the README's "A
  background service on a headless box keeps restarting"
  section](../README.md#a-background-service-on-a-headless-box-eg-raspberry-pi-keeps-restarting) —
  this is about OS keyring availability, not pairing itself.
