/// Static mirror of the agent's built-in shortcut catalog
/// (`desktop-agent/crates/devtools/src/shortcuts.rs`), used to render the Quick
/// Launch UI. The agent remains the source of truth and re-validates every
/// request; this list only drives what the phone offers.
library;

import 'dev_action.dart';

/// A catalog shortcut entry for the UI.
class ShortcutInfo {
  /// Creates an entry.
  const ShortcutInfo({
    required this.tool,
    required this.id,
    required this.description,
    required this.needsWorkspace,
  });

  /// Tool this shortcut belongs to.
  final DevTool tool;

  /// Shortcut id sent to the agent.
  final String id;

  /// Human description.
  final String description;

  /// Whether a workspace must be selected for this shortcut.
  final bool needsWorkspace;
}

/// The built-in shortcuts, mirroring the agent catalog.
const List<ShortcutInfo> kShortcutCatalog = [
  ShortcutInfo(tool: DevTool.git, id: 'status', description: 'Working tree status', needsWorkspace: true),
  ShortcutInfo(tool: DevTool.git, id: 'fetch', description: 'Fetch all remotes', needsWorkspace: true),
  ShortcutInfo(tool: DevTool.git, id: 'pull', description: 'Fast-forward pull', needsWorkspace: true),
  ShortcutInfo(tool: DevTool.git, id: 'log', description: 'Last 20 commits', needsWorkspace: true),
  ShortcutInfo(tool: DevTool.git, id: 'branches', description: 'List branches', needsWorkspace: true),
  ShortcutInfo(tool: DevTool.docker, id: 'ps', description: 'Running containers', needsWorkspace: false),
  ShortcutInfo(tool: DevTool.docker, id: 'images', description: 'Local images', needsWorkspace: false),
  ShortcutInfo(tool: DevTool.docker, id: 'compose_up', description: 'Compose up (detached)', needsWorkspace: true),
  ShortcutInfo(tool: DevTool.docker, id: 'compose_down', description: 'Compose down', needsWorkspace: true),
  ShortcutInfo(tool: DevTool.docker, id: 'compose_ps', description: 'Compose status', needsWorkspace: true),
  ShortcutInfo(tool: DevTool.kubectl, id: 'pods', description: 'Pods (all namespaces)', needsWorkspace: false),
  ShortcutInfo(tool: DevTool.kubectl, id: 'services', description: 'Services (all namespaces)', needsWorkspace: false),
  ShortcutInfo(tool: DevTool.kubectl, id: 'nodes', description: 'Cluster nodes', needsWorkspace: false),
  ShortcutInfo(tool: DevTool.kubectl, id: 'contexts', description: 'Configured contexts', needsWorkspace: false),
  ShortcutInfo(tool: DevTool.helm, id: 'list', description: 'Releases (all namespaces)', needsWorkspace: false),
];

/// Shortcuts for a given tool, in catalog order.
List<ShortcutInfo> shortcutsForTool(DevTool tool) =>
    kShortcutCatalog.where((s) => s.tool == tool).toList();
