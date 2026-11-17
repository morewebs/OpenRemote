// The primary draft keeps its familiar key. Split leaves add their identity
// so two independent inputs never share an attachment or an async operation.
export const scopedComposerKey = (chat, scope = null) => scope ? `${chat}::${scope}` : chat
export const composerChatId = key => String(key ?? '').split('::')[0]
