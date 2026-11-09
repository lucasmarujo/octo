Pod::Spec.new do |s|
  s.name           = 'OctoScanner'
  s.version        = '1.0.0'
  s.summary        = 'Camera view that reads the Octo pairing code'
  s.description    = 'Shows the camera and reads the QR code Octo shows on a computer, all on the device'
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
end
