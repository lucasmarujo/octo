const LOCAL_PAIRING = process.env.EXPO_PUBLIC_LOCAL_PAIRING === '1';

module.exports = ({ config }) => {
  if (!LOCAL_PAIRING) return config;
  return {
    ...config,
    plugins: [...config.plugins, ['expo-build-properties', { android: { usesCleartextTraffic: true } }]],
  };
};
