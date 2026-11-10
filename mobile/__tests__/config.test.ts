import app from '../app.json';

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
});
