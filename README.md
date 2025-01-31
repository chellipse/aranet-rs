# Aranet-rs

Config file is placed at ~/.config/aranet/config.toml.

Example config file:
```toml
adapter = "hci0"
mac = "ED:12:89:6C:08:37"
fahrenheit = false # optional
stream_freq = 30 # optional
prometheus_address = "127.0.0.1:8080" # optional
```

### Notes

* works via bluetooth, be sure to enable that on you're aranet4
* pairing pin entry is done via pinentry-qt
* doesn't currently support parsing data other than current_readings
* only works with current_readings on firmware >= v1.2 afaik
* only works on linux
