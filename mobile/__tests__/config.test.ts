import app from '../app.json';
import eas from '../eas.json';

declare const __dirname: string;

const { readFileSync } = jest.requireActual<{ readFileSync(path: string, encoding: 'utf8'): string }>('fs');
const { join } = jest.requireActual<{ join(...segments: string[]): string }>('path');

describe('the store builds', () => {
  it('ask only for the camera, to scan the pairing code and take a photo for a task, the vibration of the haptics and the notifications of the agents', () => {
    expect(app.expo.android.permissions).toEqual(['android.permission.CAMERA', 'android.permission.VIBRATE', 'android.permission.POST_NOTIFICATIONS']);
    expect(app.expo.android.blockedPermissions).toEqual(
      expect.arrayContaining(['android.permission.USE_BIOMETRIC', 'android.permission.USE_FINGERPRINT', 'android.permission.RECORD_AUDIO', 'android.permission.SYSTEM_ALERT_WINDOW', 'android.permission.RECEIVE_BOOT_COMPLETED']),
    );
  });

  it('show the notifications of the agents with the prompt of Octo in the status bar', () => {
    expect(app.expo.plugins).toContainEqual(['expo-notifications', { icon: './assets/notification-icon.png', color: '#35fddc' }]);
  });

  it('take the Firebase file of the push from the build and the production push environment for the stores', () => {
    const config = (env: Record<string, string>) => {
      const saved = { ...process.env };
      Object.assign(process.env, env);
      let made: { android: { googleServicesFile?: string }; plugins: unknown[] } | null = null;
      jest.isolateModules(() => {
        made = require('../app.config.js')({ config: app.expo });
      });
      process.env = saved;
      return made!;
    };
    const local = config({});
    expect(local.android.googleServicesFile).toBeUndefined();
    expect(local.plugins).toContainEqual(['expo-notifications', { icon: './assets/notification-icon.png', color: '#35fddc' }]);
    const store = config({ GOOGLE_SERVICES_JSON: '/eas/google-services.json', EAS_BUILD_PROFILE: 'production' });
    expect(store.android.googleServicesFile).toBe('/eas/google-services.json');
    expect(store.plugins).toContainEqual(['expo-notifications', { icon: './assets/notification-icon.png', color: '#35fddc', mode: 'production' }]);
  });

  it('say what the camera and the library are for when a file is attached to a task, and never ask for the microphone or the storage', () => {
    const camera = app.expo.ios.infoPlist.NSCameraUsageDescription;
    expect(camera).toContain('pairing code');
    expect(camera).toContain('attach to a task');
    expect(app.expo.plugins).toContainEqual(['expo-image-picker', { photosPermission: 'Octo opens your library only for the photos and videos you choose to attach to a task.', cameraPermission: camera, microphonePermission: false }]);
    expect(app.expo.android.blockedPermissions).toEqual(expect.arrayContaining(['android.permission.RECORD_AUDIO', 'android.permission.READ_EXTERNAL_STORAGE', 'android.permission.WRITE_EXTERNAL_STORAGE']));
    expect(app.expo.plugins.map(plugin => plugin[0])).not.toContain('expo-document-picker');
  });

  it('keep the pairing keys without asking for Face ID, which the app never uses', () => {
    expect(app.expo.plugins).toContainEqual(['expo-secure-store', { faceIDPermission: false }]);
    expect(Object.keys(app.expo.ios.infoPlist)).toEqual(['NSCameraUsageDescription']);
  });

  it('sign the iPhone build with the certificate of the team kept on this machine, never in git', () => {
    expect(eas.build.production.ios).toEqual({ credentialsSource: 'local' });
    const ignored = readFileSync(join(__dirname, '..', '.gitignore'), 'utf8').split(/\r?\n/);
    expect(ignored).toEqual(expect.arrayContaining(['credentials.json', '*.p12', '*.key', '*.mobileprovision', '*.p8']));
  });
});
