// A plugin's view in a pane - the primary pane or a split - opened from its
// tile in the sidebar's Plugins group. The plugin draws the whole view in its
// own sandboxed frame; when the plugin is off, gone, or the list is still on
// its way, the pane says so plainly.

import { PluginFrame, usePlugins } from './plugins-host.jsx'

export default function PluginView({ viewKey }) {
  const host = usePlugins()
  const info = host?.viewInfo(viewKey)
  if (!info) return <div className="cv-none">That plugin view is gone.</div>
  if (!info.plugin) return <div className="cv-none">That plugin is no longer installed.</div>
  if (!info.plugin.enabled) {
    return <div className="cv-none">{info.record?.name ?? info.plugin.name} is off. Turn it on in Plugins to open this view.</div>
  }
  if (!info.record) return <div className="plg-view" aria-busy="true" />
  if (!info.view) return <div className="cv-none">{info.record.name} has no view by that name any more.</div>
  return (
    <div className="plg-view">
      <PluginFrame
        key={`${info.plugin.id}|${info.plugin.updated_at ?? ''}|${info.view.id}`}
        pluginId={info.plugin.id}
        place={{ kind: 'view', view: info.view.id }}
        title={`${info.view.title} (${info.record.name} plugin)`}
        className="plg-view-frame"
      />
    </div>
  )
}
