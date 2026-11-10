import app from '../app.json';
import eas from '../eas.json';

declare const __dirname: string;

const { readFileSync } = jest.requireActual<{ readFileSync(path: string, encoding: 'utf8'): string }>('fs');
const { join } = jest.requireActual<{ join(...segments: string[]): string }>('path');

describe('the store builds', () => {
  it('ask only for the camera, to scan the pairing code, and the vibration of the haptics', () => {
    expect(app.expo.android.permissions).toEqual(['android.permission.CAMERA', 'android.permission.VIBRATE']);
    expect(app.expo.android.blockedPermissions).toEqual(
      expect.arrayContaining(['android.permission.USE_BIOMETRIC', 'android.permission.USE_FINGERPRINT', 'android.permission.RECORD_AUDIO', 'android.permission.SYSTEM_ALERT_WINDOW']),
    );
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
