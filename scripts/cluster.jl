using JSON

function read_payload()
    raw = read(stdin, String)
    isempty(strip(raw)) && return Dict{String, Any}("entities" => Any[], "edges" => Any[])
    return JSON.parse(raw)
end

function build_adjacency(payload)
    entities = payload["entities"]
    adj = Dict{String, Vector{String}}()
    entity_meta = Dict{String, Dict{String, Any}}()
    for raw in entities
        entity = Dict{String, Any}(raw)
        id = String(get(entity, "entity_id", ""))
        isempty(id) && continue
        adj[id] = String[]
        entity_meta[id] = entity
    end

    for raw in get(payload, "edges", Any[])
        edge = Dict{String, Any}(raw)
        src = String(get(edge, "src_id", ""))
        dst = String(get(edge, "dst_id", ""))
        if isempty(src) || isempty(dst) || src == dst || !haskey(adj, src) || !haskey(adj, dst)
            continue
        end
        push!(adj[src], dst)
        push!(adj[dst], src)
    end

    return adj, entity_meta
end

function cluster_label(members, entity_meta)
    prefixes = Dict{String, Int}()
    languages = Dict{String, Int}()
    for member in members
        meta = get(entity_meta, member, Dict{String, Any}())
        path = String(get(meta, "file_path", member))
        lang = String(get(meta, "language", "unknown"))
        parts = split(path, '/')
        prefix = length(parts) >= 2 ? join(parts[1:2], "/") : path
        prefixes[prefix] = get(prefixes, prefix, 0) + 1
        languages[lang] = get(languages, lang, 0) + 1
    end

    best_prefix = isempty(prefixes) ? "graph" : first(sort(collect(keys(prefixes)); by = key -> (-prefixes[key], key)))
    best_lang = isempty(languages) ? nothing : first(sort(collect(keys(languages)); by = key -> (-languages[key], key)))
    return isnothing(best_lang) ? best_prefix : string(best_prefix, " · ", best_lang)
end

function compute_clusters(payload)
    adj, entity_meta = build_adjacency(payload)
    visited = Set{String}()
    clusters = Any[]
    cluster_index = 0

    for id in sort(collect(keys(adj)))
        id in visited && continue
        cluster_index += 1
        stack = [id]
        members = String[]
        while !isempty(stack)
            current = pop!(stack)
            current in visited && continue
            push!(visited, current)
            push!(members, current)
            for neighbor in get(adj, current, String[])
                neighbor in visited || push!(stack, neighbor)
            end
        end

        sort!(members)
        label = cluster_label(members, entity_meta)
        cluster_id = string("cluster-", cluster_index)
        for member in members
            push!(clusters, Dict(
                "entity_id" => member,
                "cluster_id" => cluster_id,
                "label" => label,
            ))
        end
    end

    return clusters
end

payload = read_payload()
println(JSON.json(compute_clusters(payload)))
