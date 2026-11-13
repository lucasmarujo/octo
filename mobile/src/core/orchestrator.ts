import type { DeviceLink } from './link';
import type { EngineSlot, InstructionsFile } from './octo';
import {
  ACTIVITY_MAX,
  activityPage,
  activityRequest,
  chatMessage,
  clearChatMessage,
  enabledMessage,
  engineMessage,
  instructionsFile,
  instructionsRequest,
  mergeActivity,
  modeMessage,
  octoCapable,
  projectModeMessage,
  readView,
  saveInstructionsMessage,
  settled,
  turnOnMessage,
  viewRequest,
} from './octo';
import type { ActivityEntry, Mode, OrchestratorActivityMsg, OrchestratorAnswerMsg, OrchestratorInstructionsMsg, OrchestratorMsg, OrchestratorView, PhoneMsg } from './protocol';
import { Store } from './store';

export type LogState = {
  older: ActivityEntry[];
  next: number | null;
  loading: boolean;
  asked: boolean;
};

export type FileState = {
  project: string | null;
  file: InstructionsFile | null;
  saving: boolean;
};

const NO_LOG: LogState = { older: [], next: null, loading: false, asked: false };

export class Orchestrator {
  readonly view = new Store<OrchestratorView | null>(null);
  readonly log = new Store<LogState>(NO_LOG);
  readonly file = new Store<FileState | null>(null);

  constructor(private readonly link: DeviceLink) {}

  capable(): boolean {
    return octoCapable(this.link.version);
  }

  ask(): void {
    if (!this.capable() || this.link.store.get().phase !== 'live') return;
    const log = this.log.get();
    const reading = log.loading || log.asked;
    this.log.set(reading ? { ...NO_LOG, loading: true } : NO_LOG);
    this.link.send(viewRequest());
    if (reading) this.link.send(activityRequest(null));
    const wanted = this.file.get();
    if (!wanted) return;
    if (wanted.saving) this.file.set({ ...wanted, saving: false });
    if (!wanted.file) this.link.send(instructionsRequest(wanted.project));
  }

  hearView(message: OrchestratorMsg): void {
    this.view.set(settled(this.view.get(), readView(message)));
  }

  hearActivity(message: OrchestratorActivityMsg): void {
    const page = activityPage(message);
    const log = this.log.get();
    this.log.set({ older: mergeActivity(log.older, page.items), next: page.next, loading: false, asked: true });
  }

  hearInstructions(message: OrchestratorInstructionsMsg): void {
    const file = instructionsFile(message);
    const wanted = this.file.get();
    if (wanted && wanted.project === file.project) this.file.set({ project: file.project, file, saving: false });
  }

  setMode(mode: Mode): void {
    this.send(modeMessage(mode));
  }

  setProjectMode(path: string, mode: Mode | null): void {
    this.send(projectModeMessage(path, mode));
  }

  setEnabled(enabled: boolean): void {
    this.send(enabledMessage(enabled));
  }

  turnOn(primary: string, mode: Mode): void {
    this.send(turnOnMessage(primary, mode));
  }

  setEngine(slot: EngineSlot, kind: string | null): void {
    this.send(engineMessage(slot, kind));
  }

  answer(message: OrchestratorAnswerMsg): void {
    this.send(message);
  }

  say(text: string): boolean {
    const message = this.link.store.get().phase === 'live' ? chatMessage(this.view.get(), text) : null;
    if (message) this.send(message);
    return Boolean(message);
  }

  clearChat(): void {
    this.send(clearChatMessage());
  }

  openLog(): void {
    if (this.log.get().asked || this.log.get().loading) return;
    this.log.set({ ...NO_LOG, loading: true });
    this.send(activityRequest(null));
  }

  older(): void {
    const log = this.log.get();
    if (log.loading || log.next === null || log.older.length >= ACTIVITY_MAX) return;
    this.log.set({ ...log, loading: true });
    this.send(activityRequest(log.next));
  }

  readInstructions(project: string | null): void {
    this.file.set({ project, file: null, saving: false });
    this.send(instructionsRequest(project));
  }

  saveInstructions(project: string | null, text: string): boolean {
    const wanted = this.file.get();
    if (!wanted || wanted.project !== project || !wanted.file || this.link.store.get().phase !== 'live') return false;
    this.file.set({ ...wanted, saving: true });
    this.send(saveInstructionsMessage(project, text));
    return true;
  }

  closeInstructions(): void {
    this.file.set(null);
  }

  private send(message: PhoneMsg): void {
    this.link.send(message);
  }
}
