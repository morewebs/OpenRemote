# Apps and accounts across machines

## Goal

Give a person one place to see which Apps are on each of their machines, install a missing App, and add or manage several provider sign-ins for that App. A person with Personal and Work Codex accounts can choose which one a new chat uses. When that chat resumes, the same account runs it.

An App installation belongs to a machine. Each App account belongs to one installation on one machine. The OpenRemote Cloud account connects the person's devices and gives them access to their Cloud machines. Signing in to an App on one machine leaves account setup on other machines to the person. Shared access for multiple OpenRemote users is outside this design.

## Current behavior

- **Manage apps** is opened from the App picker and onboarding. It reads local capabilities and local install options. The Cloud **Machines** dialog has a separate install and sign-in flow for a remote machine.
- The daemon already routes installs to Cloud machines and has a provider sign-in relay. Login runs and capability probes are keyed by App. Chat creation and resume launch the App using its existing configuration, with no App account selection.
- Computers run Apps. Phone and web consoles can control a Cloud machine. App management uses the existing machine eligibility rules, including local use while signed out of OpenRemote Cloud.

The first release moves App management and account selection into the desktop console, including management of remote Cloud machines. Existing phone and web clients can still resume an existing chat against its saved account and start chats through the existing sign-in. Bringing the management and selection controls to those clients can follow using the same daemon contract.

## Approaches

- **Keep current sign-ins and add managed accounts.** Reuse each App's existing sign-in or setup as the default path. Give each added account its own provider-supported configuration. Existing chats, terminal usage and installations carry on. The interface needs a clear label for the existing sign-in when there are also named accounts. This is the recommended design.
- **Move every sign-in into a managed account.** Every account would follow the same setup and selection flow. Existing users would have to reconnect, and conversations created in the App's original configuration would still need a compatible resume path. Migration adds work before a person can use their existing chats.

## Manage apps

Use the same Apps view from the App picker, onboarding and a machine's details. From a composer or machine, open it on that target. From onboarding, open it on this computer. When only one machine is available, show its name without a picker. With several machines, show a compact selector above the list. Opening the view leaves the composer's selected machine intact.

Give each App one row with its name, a short status and one next action. Offer **Install** when the selected machine supports it, **Sign in** when sign-in is the next action and **Accounts** when accounts can be managed. Present the list of accounts only when the person opens an App's details. Keep versions, paths, capability flags and installer commands in details. Reuse OpenRemote's theme, focus handling and phone-sized dialog behavior.

A machine that is offline can still be selected to see its status and retry when it returns. Keep offline, loading, unsupported and failed distinct. Scope results, errors and busy states to their machine and App. An install on one machine must never change rows for another. Keep the machine name visible during installation, account setup and confirmation.

Install uses the existing installer and runtime confirmation rules. The selected daemon decides which installers are available and serializes competing installs for the same App. A second controller can observe an in-progress install. A disconnect prompts an inventory refresh or status check before retrying so a completed install is recognized. Refresh the selected machine's inventory and App picker when the install settles.

## Accounts and chats

An App's detail view lists the existing sign-in where applicable and shows managed accounts by name and status. **Add account** asks for a recognizable name and opens the App's own sign-in flow. Renaming preserves the account's identity. Expired or disconnected accounts show **Reconnect**. If sign-in is cancelled, times out or fails, show a retry action and keep the App and any saved accounts available. If a provider can't sign in remotely on that machine, direct the person to complete setup there.

Offer an account selector when starting a chat with an App that has a choice of ready accounts. Remember the last choice for that App and machine in the console as a convenience. The new chat shows which account will run it. A machine or App change clears choices from the previous context. A disconnected account that was selected requires a new explicit choice or reconnection. A sole ready managed account is shown and bound to the chat without adding a picker.

Persist the selected account's ID with the chat on the machine that runs it. Creation, resume and any automatic restart of that chat resolve the same ID. If the account is unavailable, say which account needs attention and stop before launching a driver. Reconnecting an account restores access to its chats. Removing a managed account with dependent chats first offers disconnection with a clear explanation, so its ID and configuration can be used to reconnect. The first version offers no bulk switching or permanent deletion of account data. Existing chats and clients with no account ID continue through the App's existing configuration, whose sign-in can still be changed outside OpenRemote.

Custom model endpoints keep their own authentication. Show the provider account selector only when the chat will use the App's provider, and have the daemon reject combinations that the driver cannot honor. New chats created by entry points without an account selector retain the existing sign-in default. A continuation on another App or machine needs its own account selection where available, because the source account ID only identifies a sign-in on its original installation.

## Daemon and client contract

- Use the existing machine inventory and Cloud dispatch to select the daemon. Add account support to the daemon's App capabilities so older daemons still show their install and existing sign-in paths. Account management APIs are scoped to the target machine and App. Only clients already allowed to drive a Cloud machine may manage its App accounts.
- Store managed account metadata on the owning daemon, keyed by an opaque ID. The provider retains credentials in a private, isolated configuration on that machine. Only IDs, labels, capability and status are exposed to consoles allowed to drive the machine. Credentials and private paths stay off the Cloud wire and out of receipts, events and logs. Redact sign-in output and relay only the provider instructions and one-time challenge the user needs to finish an active login.
- Use the same driver-specific configuration binding for login, credential probing, launch and resume. Keep bootstrap and child processes in that binding and control inherited provider credentials so the selected account cannot accidentally run as the machine's default user. Advertise multiple accounts only for Apps and platforms where the binding and provider sign-in can be verified. Existing machine-owned configuration remains available independently.
- Reconnect keeps the original provider identity. Verify it against a stable provider user identifier before replacing saved credentials. If it differs, keep the existing account and guide the person to **Add account** for the new identity. A driver that cannot verify the identity cannot replace credentials on an account with dependent chats.
- Validate the selected account belongs to the App, runs on the executor, and is ready before spawn. Persist and replay the binding via the existing session model. Cloud chat copies can carry a non-secret account reference. The executor resolves it and credentials stay local. Retries preserve the original target and account. Concurrent logins operate on their own account and run IDs so closing one console cannot stop another account's login.
- The first complete provider path is Codex, including a verified remote sign-in flow on supported systems. Audit other built-in Apps for isolated login, auth status, chat launch and resume. Enable their account controls as each passes those checks. Unsupported providers retain their existing setup and show that multiple accounts are unavailable only in details. Plugin Apps require a supported driver contract before offering account controls.

## Acceptance

- From machine A, select machine B in **Manage apps**, install a missing App there, and see B's refreshed status. Verify A's inventory did not change. Exercise offline, failure, retry and runtime confirmation states.
- Set up two Codex accounts on B. Run two chats with different accounts and resume both after changing the remembered choice and restarting B's daemon. Verify each provider process gets the intended configuration. Exercise the local path, a remote Windows/Linux pairing, and any other platform that advertises support.
- A disconnected or invalid account blocks launch with a recovery action. Attempting to use an account on the wrong App or machine fails at the daemon. Reconnect the same provider user successfully and verify that signing in as a different provider user cannot take over the chat. Existing chats, default sign-ins, legacy clients, onboarding and custom endpoints continue to work.
- Check machine switching during an operation, two controllers, cancelled and expired login, and an old daemon. Verify credentials never appear in responses, receipts, events or diagnostic output. Use test fixtures for routing and lifecycle behavior and verify provider-supported login and account separation with two authorized test accounts before advertising support.
- In the running Tauri app, verify the first view has one machine control and a readable App list. Check keyboard focus, narrow layout, long machine and account names, and loading, offline and empty states. A person can see where an App will install and which account a chat uses before acting.
