Pod::Spec.new do |s|
  s.name           = 'OctoLink'
  s.version        = '1.0.0'
  s.summary        = 'Encrypted link to Octo on a computer'
  s.description    = 'Opens the end-to-end encrypted channel between the Octo app and Octo on a computer'
  s.author         = 'OctoAI'
  s.homepage       = 'https://octo.marujo.dev'
  s.platforms      = {
    :ios => '16.4'
  }
  s.source         = { git: '' }
  s.static_framework = true

  s.dependency 'ExpoModulesCore'

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
  }

  s.source_files = "**/*.{h,m,mm,swift,hpp,cpp}"
  s.exclude_files = "Tests/"
end
