import { createContext, useContext } from 'react';

import type { Device } from '../../core/devices';
import type { DeviceLink } from '../../core/link';
import type { Workspace } from '../../core/workspace';

export type DeviceScope = {
  device: Device;
  link: DeviceLink;
  workspace: Workspace;
};

export const DeviceContext = createContext<DeviceScope | null>(null);

export function useDevice(): DeviceScope {
  const scope = useContext(DeviceContext);
  if (!scope) throw new Error('this component needs a paired computer around it');
  return scope;
}
