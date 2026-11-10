const path = require('path');
const { getDefaultConfig } = require('expo/metro-config');

const config = getDefaultConfig(__dirname);

config.watchFolders = [...(config.watchFolders || []), path.resolve(__dirname, '..', 'src', 'pwa', 'web'), path.resolve(__dirname, '..', 'desktop', 'ui', 'js')];
config.resolver.nodeModulesPaths = [path.resolve(__dirname, 'node_modules')];

module.exports = config;
