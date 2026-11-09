module.exports = {
  preset: 'jest-expo',
  modulePaths: ['<rootDir>/node_modules'],
  testMatch: ['**/__tests__/**/*.test.ts', '**/__tests__/**/*.test.tsx'],
  testPathIgnorePatterns: ['/node_modules/', '/android/', '/ios/'],
};
