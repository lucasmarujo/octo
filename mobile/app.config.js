const LOCAL_PAIRING = process.env.EXPO_PUBLIC_LOCAL_PAIRING === '1';
const GOOGLE_SERVICES = process.env.GOOGLE_SERVICES_JSON;
const STORE = process.env.EAS_BUILD_PROFILE === 'production';
const NOTIFICATIONS = 'expo-notifications';

function storePush(plugin) {
  if (!STORE || !Array.isArray(plugin) || plugin[0] !== NOTIFICATIONS) return plugin;
  return [NOTIFICATIONS, { ...plugin[1], mode: 'production' }];
}

module.exports = ({ config }) => {
  const plugins = config.plugins.map(storePush);
  return {
    ...config,
    android: GOOGLE_SERVICES ? { ...config.android, googleServicesFile: GOOGLE_SERVICES } : config.android,
    plugins: LOCAL_PAIRING ? [...plugins, ['expo-build-properties', { android: { usesCleartextTraffic: true } }]] : plugins,
  };
};
