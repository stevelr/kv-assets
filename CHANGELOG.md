
v0.2.3

- split out KV as a separate (and trivial) Worker KV client with get and
  put methods. This is a non-breaking change for KVAssets

- changed some of the Enum Error values so that Error can implement Clone + Send

v0.2.2  2021-01-23 

- fixed structure definition for response to put_kv_value.
- more verbose error logging
