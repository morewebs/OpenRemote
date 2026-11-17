// Return the delivery handle as well as creating a session, so a caller can
// navigate immediately and still restore a rejected first message.
export function firstMessageDelivery(api, id, text, mentioned, attachments, onFailure) {
  if (!String(text ?? '').trim() && !attachments.length) return Promise.resolve()
  return api.prompt(id, String(text ?? '').trim(), mentioned, attachments).catch(err => {
    onFailure(err)
    throw err
  })
}
