import * as SecureStore from 'expo-secure-store';

const OPTIONS: SecureStore.SecureStoreOptions = { keychainAccessible: SecureStore.AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY };

export async function load<T>(name: string): Promise<T | null> {
  const text = await SecureStore.getItemAsync(name, OPTIONS);
  if (text === null) return null;
  try {
    return JSON.parse(text) as T;
  } catch {
    return null;
  }
}

export function save(name: string, value: unknown): Promise<void> {
  return SecureStore.setItemAsync(name, JSON.stringify(value), OPTIONS);
}
