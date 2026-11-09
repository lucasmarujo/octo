import { NativeModule, requireNativeModule, requireNativeView } from 'expo';
import type { ComponentType } from 'react';
import type { ViewProps } from 'react-native';

export type Scanned = {
  nativeEvent: {
    text: string;
  };
};

export type ScanFailed = {
  nativeEvent: {
    reason: 'denied' | 'unavailable';
  };
};

export type ScannerViewProps = ViewProps & {
  onScanned: (event: Scanned) => void;
  onFailed: (event: ScanFailed) => void;
};

declare class OctoScannerModule extends NativeModule<{}> {
  requestAccess(): Promise<boolean>;
}

export const ScannerView: ComponentType<ScannerViewProps> = requireNativeView('OctoScanner');

export default requireNativeModule<OctoScannerModule>('OctoScanner');
