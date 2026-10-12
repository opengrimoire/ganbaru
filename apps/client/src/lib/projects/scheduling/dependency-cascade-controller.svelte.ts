import { previewProjectDependencyCascade, type ProjectDependencyCascadePreview } from "$lib/api/project-cascade";

/** Keep native review results scoped to the selected project and the latest explicit refresh. */
export class ProjectDependencyCascadeController {
  open = $state(false);
  loading = $state(false);
  applying = $state(false);
  error = $state<string | null>(null);
  preview = $state<ProjectDependencyCascadePreview | null>(null);
  private generation = 0;

  constructor(
    private readonly readProjectId: () => string | null,
    private readonly applyPreview: (preview: ProjectDependencyCascadePreview) => Promise<void>,
    private readonly loadPreview = previewProjectDependencyCascade,
  ) {}

  reset(): void {
    this.generation += 1;
    this.open = false;
    this.loading = false;
    this.applying = false;
    this.error = null;
    this.preview = null;
  }

  async review(): Promise<void> {
    const projectId = this.readProjectId();
    if (!projectId || this.applying) return;
    const generation = ++this.generation;
    this.open = true;
    this.loading = true;
    this.error = null;
    this.preview = null;
    try {
      const preview = await this.loadPreview(projectId);
      if (this.generation === generation && this.readProjectId() === projectId) this.preview = preview;
    } catch (error) {
      if (this.generation === generation && this.readProjectId() === projectId) this.error = error instanceof Error ? error.message : String(error);
    } finally {
      if (this.generation === generation) this.loading = false;
    }
  }

  async apply(): Promise<void> {
    const preview = this.preview;
    if (!preview || this.loading || this.applying || preview.projectId !== this.readProjectId()
      || preview.conflicts.length > 0 || preview.items.length === 0) return;
    const generation = this.generation;
    this.applying = true;
    this.error = null;
    try {
      await this.applyPreview(preview);
      if (this.generation === generation) this.reset();
    } catch (error) {
      if (this.generation === generation) this.error = error instanceof Error ? error.message : String(error);
    } finally {
      if (this.generation === generation) this.applying = false;
    }
  }
}
