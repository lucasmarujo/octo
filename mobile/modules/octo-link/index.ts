import { NativeModule, requireNativeModule } from 'expo';

export type LinkReady = {
  id: string;
};

export type LinkMessage = {
  id: string;
  text: string;
};

export type LinkClosed = {
  id: string;
  code: number;
  reason: string;
};

type OctoLinkEvents = {
  onReady(event: LinkReady): void;
  onMessage(event: LinkMessage): void;
  onClosed(event: LinkClosed): void;
};

declare class OctoLinkModule extends NativeModule<OctoLinkEvents> {
  open(id: string, url: string, key: string, agent: string): void;
  send(id: string, text: string): void;
  queued(id: string): number;
  close(id: string): void;
}

export default requireNativeModule<OctoLinkModule>('OctoLink');
