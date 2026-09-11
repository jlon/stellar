const fs = require('fs');
const os = require('os');
const path = require('path');

// Resolve a Chrome binary synchronously: system chrome first, then the
// puppeteer download cache (puppeteer's executablePath() is async now).
function findChromeBinary() {
  const candidates = [
    '/usr/bin/google-chrome',
    '/usr/bin/google-chrome-stable',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
  ];
  for (const c of candidates) {
    if (fs.existsSync(c)) return c;
  }
  const cacheDir = path.join(os.homedir(), '.cache/puppeteer/chrome');
  if (fs.existsSync(cacheDir)) {
    for (const ver of fs.readdirSync(cacheDir)) {
      const bin = path.join(cacheDir, ver, 'chrome-linux64', 'chrome');
      if (fs.existsSync(bin)) return bin;
    }
  }
  return undefined;
}

process.env.CHROME_BIN = findChromeBinary();

// Karma configuration file generated for Angular CLI unit tests
module.exports = function (config) {
  config.set({
    basePath: '',
    frameworks: ['jasmine', '@angular-devkit/build-angular'],
    plugins: [
      require('karma-jasmine'),
      require('karma-chrome-launcher'),
      require('karma-jasmine-html-reporter'),
      require('karma-coverage-istanbul-reporter'),
      require('@angular-devkit/build-angular/plugins/karma'),
    ],
    client: {
      jasmine: {
        random: false,
      },
      clearContext: false,
    },
    coverageIstanbulReporter: {
      dir: require('path').join(__dirname, './coverage/stellar'),
      subdir: '.',
      reporters: [
        { type: 'html' },
        { type: 'text-summary' },
      ],
    },
    reporters: ['progress', 'kjhtml'],
    port: 9876,
    colors: true,
    logLevel: config.LOG_INFO,
    autoWatch: false,
    browsers: ['ChromeHeadlessNoSandbox'],
    customLaunchers: {
      ChromeHeadlessNoSandbox: {
        base: 'ChromeHeadless',
        flags: ['--no-sandbox', '--disable-setuid-sandbox'],
      },
    },
    browserNoActivityTimeout: 120000,
    browserDisconnectTimeout: 120000,
    browserDisconnectTolerance: 2,
    captureTimeout: 120000,
    singleRun: true,
    restartOnFileChange: false,
  });
};

