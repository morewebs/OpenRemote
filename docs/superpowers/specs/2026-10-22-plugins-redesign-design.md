# Plugins View Redesign Design Document

## Intent and Why
The OpenRemote Plugins view currently feels disjointed, cramped, and unfinished: it enforces an artificial 880px width limit, renders a 300px inspection rail that sits empty until clicked, uses unstyled browser `<select>` controls, and dumps raw git error strings across the screen when a fetch fails. This redesign transforms the Plugins page into a clean, balanced, high-density catalog that matches OpenRemote's dark machine-room aesthetic, providing immediate visual anchors, clear category grouping, and in-place actions.

## Goals, Non-Goals, and Anti-Goals

### Goals
- Replace the 880px-constrained split layout with a full-width responsive 2-column catalog grid.
- Provide immediate visual identity for every plugin using a 36×36px icon tile (official brand marks for recognized tools, styled monogram tiles for community plugins).
- Group Discover plugins under category sections (e.g., Development, Security, Productivity) by default, showing up to 6 items per category with a "Show more" / "Show less" toggle.
- Transition seamlessly from categorized sections to a flat, relevance-sorted 2-column grid when searching or filtering.
- Allow direct action with an in-place `Install` button that opens `InstallPluginModal` immediately, while clicking the card body opens a dedicated `PluginDetailModal`.
- Wrap marketplace catalog fetch errors in a contained, dismissible alert card with a retry button.
- Unify the layout and styling across all three tabs (`Discover`, `Installed`, `Marketplaces`) so the entire Plugins feature feels cohesive and spacious.

### Non-Goals
- Altering the underlying daemon plugin staging, installation, or marketplace sync protocols (the backend APIs remain unchanged).
- Changing how manual MCP servers are managed in Settings.

### Anti-Goals
- Do not introduce arbitrary new accent colors or floating drop shadows. Adhere strictly to the design contract (`#0A0C0F` canvas, 1px lines, Inter/Geist Mono, amber `#DCA24B` accent, green `#4CB487` status).
- Do not make the catalog cards feel like oversized marketing banners or generic SaaS cards. Keep the information density high, crisp, and machine-like.

## Constraints
- Must adhere to OpenRemote's design system (no em dashes, Segoe MDL2 / Phosphor icon conventions, reduced motion support via `@media` and `html[data-reduce-motion]`).
- Must operate cleanly within Tauri and browser environments.
- Must preserve all existing plugin capabilities, powers disclosures, and clash warnings.

## Approaches Considered

1. **Approach 1: Categorized 2-Column Catalog with Dedicated Detail Modal (Selected)**
   - Replaces the 880px cap and 300px rail with a full-canvas 2-column grid.
   - Categorized sections with a 6-item threshold and "Show more (N)" expander.
   - Dedicated `PluginDetailModal` for inspectability; direct `Install` button on card for quick action.
   - Selected because it resolves the dead-space problem, matches the user's reference layout, and keeps inspection focused without disrupting grid flow.

2. **Approach 2: In-Place Accordion Card Expansion (Rejected)**
   - Expanded cards in place within the grid. Rejected because variable height expansion in a 2-column CSS grid causes jarring layout shifts and misalignment.

3. **Approach 3: Flat List with Category Pills Only (Rejected)**
   - A single flat list without section headers. Rejected because it hides the variety of plugins across categories and forces unnecessary clicking.

## Detailed Design

### 1. Catalog Grid & Grouping (`PluginDiscover.jsx`)
- The Discover tab spans the full panel width (`width: 100%`).
- **Browsing Mode (no active search query):**
  - Plugins are grouped dynamically using existing catalog categories (`categories(entries)` in `plugins.js`), mapping recognized tags into title-cased sections (e.g. `Development`, `Security`, `Productivity`, `Design`) and bucketing uncategorized entries into `General`.
  - Empty categories are excluded.
  - Each category section features a section heading (`pn-section`), a count, and a 2-column grid (`pg-grid-2col`).
  - If a category contains more than 6 plugins, it renders the first 6 and displays a "Show {N} more" button. When expanded, it displays "Show less".
- **Search/Filter Mode (when `query` or `category` or `marketplace` filter is active):**
  - Renders a unified 2-column grid of all matching plugins.
  - Sorted by relevance when a text query is present (exact name match > prefix match > description match > author/marketplace); sorted alphabetically when only filtering by category or marketplace.
  - Section headers are hidden in favor of a clean result count: `Showing {shown.length} plugins`.

### 2. Card Anatomy & Visual Identity (`PluginCard.jsx` & `PluginIcon.jsx`)
- Each card (`pg-card`) is an accessible, interactive tile:
  - **Left:** 36×36px `PluginIcon` tile (`border-radius: 8px`).
    - Recognized tool/author brands (e.g., GitHub, GitLab, Docker, Android, AWS, Anthropic, Python) render official monochrome/brand marks or Phosphor icons.
    - Other plugins render a clean 2-letter monogram badge derived from the plugin's name (e.g. "42crunch" -> "42", "agent-sdk" -> "AS") on a cool-grey tile (`rgba(255, 255, 255, 0.05)`, 1px border), with category-specific icon fallbacks.
  - **Middle:**
    - Name: 13.5px / 600 weight, primary text color.
    - Description: 2-line clamped secondary text (`--text-2`), 12.5px.
    - Meta row: Author / Marketplace source, powers indicator chips (e.g., "Non-sandboxed" warning chip if applicable).
  - **Right:** Action button:
    - If not installed: `Install` button (amber accent or ghost button with hover highlight).
    - If update available: `Update` button (amber accent).
    - If already installed: `Installed` status indicator or check badge.
- Clicking the `Install` or `Update` button triggers `onInstall(entry)` directly.
- Clicking the card body opens `PluginDetailModal`.

### 3. Dedicated Plugin Detail Modal (`PluginDetailModal.jsx`)
- Follows the OpenRemote modal family (`dv-modal` / `devices.css`):
  - Backdrop, Esc key to dismiss, X close button.
  - Header: 40×40px `PluginIcon`, display name, technical ID in Geist Mono, author, version.
  - Body:
    - Full unclamped description.
    - Facts grid: Runtime (`RUNTIME_LABEL`), Marketplace source, Claude parts carried.
    - Powers & Security section: Explicit list of powers (`POWER_LABEL`), non-sandboxed execution warning if applicable.
    - Homepage link button (opens external URL).
  - Footer: `Close` button and primary action (`Install` / `Update` / `Installed`).
  - Clicking `Install` or `Update` in `PluginDetailModal` dismisses `PluginDetailModal` and immediately delegates to `InstallPluginModal` for safe staging, command confirmation, and execution.

### 4. Toolbar & Filter Controls (`pg-toolbar`)
- Replaces native `<select>` dropdowns with styled custom controls:
  - Search input with clear button and magnifying glass icon.
  - Category selector: clean dropdown or pill filter.
  - Marketplace source selector: clean dropdown.
  - Refresh button: icon button with spinning indicator while fetching.
- **Error Banner:**
  - Marketplace fetch failures are rendered in a `.pg-error-card`:
    - Warning icon, headline `Couldn't refresh marketplace {name}`.
    - Retry button to trigger `refreshMarketplace(name)`.
    - Dismiss button to dismiss the banner.
    - Collapsible toggle to inspect raw error details.

### 5. Alignment across `PluginsView.jsx` and `PluginMarketplaces.jsx`
- Remove the `width: min(880px, 100%)` constraint on tabs, `.pn-list`, `.pg-tabs`, and `.pg-filters`.
- Tabs render as clean pill tabs at the top of `PluginsView.jsx`.
- Installed plugins and Built-in harnesses in `Installed` tab retain their full action row structure (`On/Off` toggle switch, `Update`, `Remove` confirmation), expanded to 100% width with consistent card padding and typography matching the refreshed Discover tab.

## Builder Calls (Decided)
- **Installed Tab Layout:** Keeps the list row structure (`.pn-row`) to accommodate the multi-step toggle and remove controls safely, but expands to 100% width to match the new canvas geometry.
- **Category Taxonomy:** Derives categories dynamically from catalog entries via `categories(entries)` with title-casing, grouping unclassified entries into `General` and omitting empty categories.
- **Monogram Initials:** Initials are derived from the plugin's name (e.g. "CL" for Code Linter), rendered on a quiet cool-grey tile with category-specific icon fallbacks.
- **Relevance Ranking:** Search scores matches by name prefix/exact match first, then description, ensuring immediate relevance.
- **Modal Delegation:** `PluginDetailModal` handles read-only inspection; clicking `Install` delegates to `InstallPluginModal` to ensure staging safety and command verification are never bypassed.
