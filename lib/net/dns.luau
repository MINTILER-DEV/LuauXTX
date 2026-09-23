local native = ...
local Promise = require("async/promise")
local dns = {}
function dns.lookup(host) return Promise.new(function(resolve, reject) native.dns_lookup(host, resolve, reject) end) end
return dns
