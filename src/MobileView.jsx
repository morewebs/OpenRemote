// Mobile: the phone companion page. A phone drives the account's chats -
// it runs none itself - so this is where the phones are: what signing in
// on one gets you, and the phones already in the Cloud, each opening the
// same device detail every other device gets.
//
// Pairing by QR code sits above all of that and depends on none of it: a
// phone scans a code from this computer, on this network, with no account.

import { useState } from 'react'
import { CaretRight, DeviceMobile } from '@phosphor-icons/react'
import { isMobile } from './platform.js'
import MachineModal from './MachineModal.jsx'
import CloudStateRow from './CloudStateRow.jsx'
import PairingSection from './PairingSection.jsx'
import { useConsole } from './state/console.jsx'
import { usePrivacyMode } from './privacy.js'
import { isPhone, platformLabel, selfLabel, signedIn } from './cloud.js'
import './devices.css'
import './panels.css'

function detail(device) {
  return [platformLabel(device.platform), device.app_version].filter(Boolean).join(' · ')
}

// An account with no phones used to leave the grid empty under a heading
// that counts zero, which reads as a page that stopped halfway. This row
// is the account half of the story, and it deliberately never repeats the
// pairing panel above it: a phone paired to this computer over its network
// and a phone signed in to the Cloud are different things, and one empty
// state mistaken for the other sends a user to pair a phone they already
// paired. No control here: the first step is on the phone itself, so a
// button in this row would have nothing to do.
function NoPhonesYet() {
  // On a phone this screen IS the phone app: telling the person to install the app they
  // are holding, and calling the computer "this computer", was written for a desktop.
  const onPhone = isMobile
  return (
    <div className="pn-list pn-list--tight">
      <div className="pn-row">
        <div>
          <div className="pn-name">No phones in your Cloud yet.</div>
          <div className="pn-detail">
            {onPhone
              ? 'This phone lists the chats you sync and can drive your machines. Sign in above to see them.'
              : 'A phone signed in to your Cloud lists the chats you sync and can drive your machines. Install OpenRemote on the phone and sign in with the same account.'}
          </div>
        </div>
      </div>
    </div>
  )
}

// The list has not answered yet, so this page cannot say whether a phone is in
// the account. Unknown is not empty: the empty row above is a claim about the
// account, and this one is only a statement about the read.
function CheckingPhones() {
  return (
    <div className="pn-list pn-list--tight">
      <div className="pn-row">
        <div>
          <div className="pn-name">Checking your Cloud…</div>
          <div className="pn-detail">
            {isMobile
              ? 'Looking for the phones signed in to your account. This phone is listed here once the answer arrives.'
              : 'Looking for phones signed in to your account. A phone paired to this computer is listed above either way.'}
          </div>
        </div>
      </div>
    </div>
  )
}

export default function MobileView({ onOpenChat }) {
  const privacy = usePrivacyMode()
  const { devices, devicesAnswered, thisDevice, cloud, connection } = useConsole()
  const [openId, setOpenId] = useState(null)
  const inCloud = signedIn(cloud)
  const phones = (devices ?? []).filter(isPhone)
  const open = phones.find((d) => d.id === openId) ?? null
  // A poll that failed leaves the list unanswered for as long as the failure lasts, so
  // "Checking your Cloud…" was the only answer a phone ever got: a slow read and a broken
  // one looked the same. The console's own connection words are shown instead.
  const stalled = !devicesAnswered && connection?.state === 'error'

  return (
    <div className="devices">
      <header className="dv-head">
        <div>
          <h1 className="dv-title">Mobile</h1>
          <p className="dv-meta">
            {!inCloud
              ? 'OpenRemote on your phone'
              : devicesAnswered
                ? `${phones.length} ${phones.length === 1 ? 'phone' : 'phones'} in your Cloud`
                : 'Checking your Cloud…'}
          </p>
        </div>
      </header>

      <PairingSection />

      {!inCloud ? (
        <div className="pn-list pn-list--tight">
          <CloudStateRow one="phone" />
        </div>
      ) : !devicesAnswered ? (
        stalled ? (
          <div className="pn-list pn-list--tight">
            <div className="pn-row">
              <div>
                <div className="pn-name">Your Cloud did not answer.</div>
                <div className="pn-detail">
                  {connection?.error ?? 'The console could not read the account.'} Your phones are
                  not lost; this screen just cannot list them right now.
                </div>
              </div>
            </div>
          </div>
        ) : (
          <CheckingPhones />
        )
      ) : phones.length === 0 ? (
        <NoPhonesYet />
      ) : (
        <div className="dv-grid">
          {phones.map((device) => (
            <button
              key={device.id}
              className="dv-card"
              onClick={() => setOpenId(device.id)}
              title={privacy ? undefined : device.name}
            >
              <span className="dv-icon">
                <DeviceMobile size={20} weight="duotone" />
              </span>
              <span className="dv-name">
                <span className="pv">{device.name}</span>
                {device.id === thisDevice && <span className="dv-badge">{selfLabel(device)}</span>}
              </span>
              <span className="dv-detail">{detail(device)}</span>
              <span className="dv-status">
                <span className={`dv-dot dv-dot--${device.online ? 'online' : 'offline'}`} />
                {device.online ? 'Online' : 'Offline'}
              </span>
              {/* The card is the only way into a device on this screen, and its only
                  affordance was a hover border: on a touch screen there is no hover, so it
                  read as a status tile and the way in was invisible. */}
              <span className="dv-open" aria-hidden="true">
                <CaretRight size={12} weight="bold" />
              </span>
            </button>
          ))}
        </div>
      )}

      {open && (
        <MachineModal device={open} onOpenChat={onOpenChat} onClose={() => setOpenId(null)} />
      )}
    </div>
  )
}
