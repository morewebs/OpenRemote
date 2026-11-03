# Security policy

OpenRemote runs AI coding agents with your permissions on your machine. That
makes its own trust boundary worth attacking, and reports about it welcome.

## Supported versions

The newest published release is supported. Older builds are not patched; the
fix is to upgrade. The current line is 0.4.x (0.4.8 as this is written).

## Reporting a vulnerability

Use GitHub's private advisory form:

**https://github.com/morewebs/OpenRemote/security/advisories/new**

That keeps the report private to you and the maintainer while it is being
fixed. If the form is not available to you, message
[Telegram](https://t.me/iWebbIO) and ask for an advisory to be opened - please
do not open a public issue with the details.

A useful report says: the version and platform, what you did, what happened,
and what you expected. A proof of concept is welcome; a working exploit against
someone else's machine is not.

## What to expect

OpenRemote is maintained by one person. In practice:

- an acknowledgement within three working days;
- an assessment - in scope or not, and why - within ten;
- a fix, a mitigation, or a written explanation of why neither is coming.

There is no bug bounty. Credit in the advisory and the release notes is offered
if you want it, and withheld if you do not. Please give a fix a reasonable
window before publishing; if the project goes quiet for more than 90 days, say
what you know.

## Scope

In scope:

- the daemon's local HTTP API, and the peer routes between devices;
- the Noise channel that carries Cloud mode, and the relay's handling of
  ciphertext - including anything that would let the relay read a chat;
- harness drivers, plugin installation, and the screen that shows what a plugin
  may run;
- the update and checksum path, and the signed Android build;
- anything that lets a peer device do more than the documented trust model says
  it can, or lets a plugin reach ambient credentials.

Out of scope:

- the harnesses themselves and their vendors' APIs - report those to Anthropic,
  OpenAI and the rest;
- an attacker who already has your OS account, your unlocked device, or your
  `~/.openremote` directory;
- the limits listed below, which are documented rather than unknown.

## Limits that are known, not vulnerabilities

These are stated plainly on the product site and in the trust model, and a
report that they exist is not a finding:

- **The apps run as you.** There is no container or VM boundary per chat, so a
  command the agent runs has your permissions. Use a disposable VM or a
  separate OS account on machines that hold anything you would not hand over.
- **Authority is account-wide.** A device of your account can drive a machine
  and answer an approval. Per-agent identity, scoped tokens and operator-only
  approvals do not exist yet.
- **The desktop installers are unsigned.** SHA256SUMS is published on every
  release; the Android APK is signed at build time.
- **Chats are stored locally, not hash-chained.** There is no tamper-evident
  audit log and no actor attribution on events yet.

The full route-by-route model is in
[`docs/cloud-trust-model.md`](docs/cloud-trust-model.md). A report that
*exceeds* one of those limits - a peer answering an approval it should not, a
plugin reading an inherited secret, a chat surviving where it should not - is
exactly the kind of finding wanted.

## Safe harbour

Research in good faith against your own machines and your own account: this
project will not pursue you for it. Do not test against other people's
instances, do not take data that is not yours, and do not run destructive tests
on hardware you do not own.

## Repository settings worth checking

Two GitHub features do most of the work here and are configuration, not code:
private vulnerability reporting must be enabled for the advisory link above to
work, and secret scanning with push protection should be on. Both are free on a
public repository.
