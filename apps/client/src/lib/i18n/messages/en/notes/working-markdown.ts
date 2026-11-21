export const workingMarkdown = {
  workingMarkdown: {
    title: "Working folders",
    refreshTree: "Refresh Markdown files",
    truncated: "The folder scan reached a safety limit. Some Markdown files are not shown.",
    unavailableFolders: (count: number) => count === 1
      ? "One project working folder is unavailable on this device."
      : `${count} project working folders are unavailable on this device.`,
    unsaved: "Unsaved changes",
    edit: "Edit",
    preview: "Preview",
    refreshFile: "Refresh",
    rawEditor: "Raw Markdown editor",
    openExternally: "Open externally",
    copyPath: "Copy relative path",
    saving: "Saving",
    conflict: "This file changed outside Ganbaru. Your local text was not overwritten.",
    reloadRemote: "Reload file",
    copyLocal: "Copy local text",
    discardConfirm: "Discard unsaved changes to this Markdown file?",
    discardConfirmAction: "Discard",
    treeLoadFailed: "Markdown files could not be listed.",
    loadFailed: "This Markdown file could not be opened.",
    saveFailed: "This Markdown file could not be saved.",
  },
} as const;
