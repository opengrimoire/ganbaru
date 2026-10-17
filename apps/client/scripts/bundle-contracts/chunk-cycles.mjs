/**
 * Find groups of emitted chunks that statically import each other in a cycle.
 *
 * Production chunks wrap their modules in lazy initializers, so a static import cycle between chunks can run
 * one chunk's top level before the other chunk has assigned its initializers. That fails only in the
 * production bundle, where the module graph is split differently than in development.
 *
 * @param {readonly { fileName: string, imports: readonly string[] }[]} chunks Emitted chunk graph.
 * @returns {string[][]} Each cyclic group as sorted chunk file names, including self imports.
 */
export function findStaticChunkCycles(chunks) {
  const importsByFile = new Map(chunks.map((chunk) => [chunk.fileName, chunk.imports]));
  const indexByFile = new Map();
  const lowLinkByFile = new Map();
  const stack = [];
  const onStack = new Set();
  const cycles = [];
  let nextIndex = 0;

  // Iterative Tarjan strongly connected components, so deep chunk graphs cannot overflow the call stack.
  for (const start of importsByFile.keys()) {
    if (indexByFile.has(start)) continue;
    const frames = [{ file: start, edge: 0 }];
    indexByFile.set(start, nextIndex);
    lowLinkByFile.set(start, nextIndex);
    nextIndex += 1;
    stack.push(start);
    onStack.add(start);

    while (frames.length > 0) {
      const frame = frames[frames.length - 1];
      const imports = (importsByFile.get(frame.file) ?? []).filter((file) => importsByFile.has(file));
      if (frame.edge < imports.length) {
        const next = imports[frame.edge];
        frame.edge += 1;
        if (!indexByFile.has(next)) {
          indexByFile.set(next, nextIndex);
          lowLinkByFile.set(next, nextIndex);
          nextIndex += 1;
          stack.push(next);
          onStack.add(next);
          frames.push({ file: next, edge: 0 });
        } else if (onStack.has(next)) {
          lowLinkByFile.set(frame.file, Math.min(lowLinkByFile.get(frame.file), indexByFile.get(next)));
        }
        continue;
      }

      frames.pop();
      const parent = frames[frames.length - 1];
      if (parent) {
        lowLinkByFile.set(parent.file, Math.min(lowLinkByFile.get(parent.file), lowLinkByFile.get(frame.file)));
      }
      if (lowLinkByFile.get(frame.file) !== indexByFile.get(frame.file)) continue;

      const group = [];
      let member;
      do {
        member = stack.pop();
        onStack.delete(member);
        group.push(member);
      } while (member !== frame.file);
      if (group.length > 1 || imports.includes(frame.file)) cycles.push(group.sort());
    }
  }
  return cycles;
}

/**
 * Describe static chunk cycles as contract failures.
 *
 * @param {readonly { fileName: string, imports: readonly string[] }[]} chunks Emitted chunk graph.
 * @returns {string[]} One failure message per cyclic group.
 */
export function staticChunkCycleFailures(chunks) {
  return findStaticChunkCycles(chunks).map(
    (group) => `emitted chunks import each other in a cycle: ${group.join(", ")}`,
  );
}
