-- Record the class of every internal host-tool call, so call limits are counted per class and a
-- teammate that spent its reads can still ask or report.
ALTER TABLE chat_host_tool_invocations ADD COLUMN tool_class TEXT NOT NULL DEFAULT 'read' CHECK (
    tool_class IN ('read', 'edit', 'destructive', 'delegate', 'ask', 'report', 'secret', 'schedule')
);
